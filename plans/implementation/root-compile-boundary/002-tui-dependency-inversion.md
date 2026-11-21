# Root Compile Boundary Milestone 002 — TUI dependency inversion

Status: implemented — conditionally closed. Hosted before/after `cfg(test)` observation recorded from runs `44837892044` and `44937196668`. The after job failed on a deleted `src/main.rs` path; hosted confirmation of the retarget is still absent. See `plans/closure/root-compile-boundary/002-status.md`. ADR-0064 accepted 2026-11-21 by the project maintainer

Repository baseline: `b8a6215c885289654d5a8ee951bcccde401a529e`

The planning worktree has unrelated unstaged edits to `.config/nextest.toml`, `.github/workflows/ci.yml`, `.opencode/skills/testing-ci/SKILL.md`, `AGENTS.md`, `architecture/testing.md`, and `scripts/verify.sh`, plus untracked `scripts/annotate_nextest_junit.py` and `scripts/iter_selection.py`. Those drafts are not the baseline. Do not stage them. If a documentation sentence this milestone must correct sits in a dirty `AGENTS.md` or testing skill, edit that sentence from the baseline version or stop. Do not absorb the drafts.

Source roadmap:

- `plans/subsystems/root-compile-boundary-roadmap.md#milestone-2--tui-dependency-inversion`

Long-term requirements:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#44-frontends-render-projections`
- `plans/001-terminology-and-domain-model.md#13-protocol-terms`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Applicable ADRs:

- `plans/adrs/ADR-0064-root-compile-unit-ownership.md` (accepted 2026-11-21)
- ADR-0010 (accepted) — desktop stays a daemon client; this plan does not make the TUI crate a second authority
- ADR-0052 (accepted) — the daemon owns the command catalog; this plan does not move a command into the frontend

Primary class: infrastructure

## 1. Objective

Move `src/tui` into a library crate that depends on the root service library, and move the `codegg` product bin to a package that may depend on both, so the root library no longer compiles the TUI and does not depend on it.

## 2. Why this milestone is ready

ADR-0064 was accepted on 2026-11-21. That was the operational dependency, and it is satisfied. Milestone 001 is not a hard dependency. The frontend projection rule and the daemon-owned command catalog are stable interface dependencies. This plan must not reopen them.

## 3. Current implementation evidence

At the baseline, `src/tui` is 236,317 lines of the root unit's 663,456. `src/lib.rs` declares `pub mod tui`. A `pub use` of an extracted TUI crate from that file would put the crate back in the root unit and cancel the compile win. Cargo dependencies are package-scoped, so the root package also cannot take a normal, optional, or dev dependency on a TUI crate that itself depends on the root. That includes the tests.

Production reverse edges, confirmed against the tree:

- `src/theme/registry.rs` and `src/theme/target.rs` import `crate::tui::theme::Theme`. `src/theme/schema.rs` names that type only in a doc comment. `SemanticTheme` is already the frontend-neutral type. `crate::theme` is consumed by `src/theme` and `src/tui` only. Theme is 3,999 lines.
- `SecurityReviewTaskState` in `src/security/workflow/receipt.rs` stores `crate::tui::task_lifecycle::TuiTaskId`. Production users are `src/tui/app/mod.rs` (the `App` field) and `src/tui/commands/security.rs` (the constructor). The type's own comment says the `App` stores it so the TUI can abort a background review.
- `src/client/attach.rs` imports `crate::tui`. `src/lib.rs` compiles `client` only with `feature = "server"`. `src/server` does not import `crate::tui`. Leaving attach in the root library forces the forbidden edge as soon as the TUI is a separate crate.
- `src/main.rs` is the implicit `codegg` bin and has three `crate::tui` references. It is also the CLI bootstrap that guards treat as `src/main.rs`: daemon cwd allow, scheduler bypass, headless workspace binding, request-ledger writers, config provenance, nested-runtime block, scratch reclaim. Those guards must follow the file. They must not be weakened, and the file must not stay in the root package.

Test-only reverse edges, which cannot become a root dev-dependency:

- `src/core/daemon.rs` tests import `crate::tui::commands` schedule helpers. The import is inside `#[cfg(test)] mod tests`.
- `src/core/transport/daemon_socket_integration_tests.rs` constructs `crate::tui::app::App`.
- `src/security/workflow/mod.rs` imports `SecurityReviewDialog` inside `#[cfg(test)] mod tests`, which begins at the module's test gate.
- The root integration family `tests/tui_render` references `codegg::tui`. After the cut, `codegg::tui` must not exist. The family moves. It is not dropped from `cargo nextest run --workspace --profile ci`.

`src/agent`, `src/tool`, and `src/scheduler` do not import `crate::tui`. Do not split them in this milestone.

The hosted job compiles the workspace, including this unit, with no extra features. Extracting the TUI does not authorize removing it from that job. The speedup, if any, is a smaller root rustc unit plus a separate TUI crate, still both built by `--workspace`. Line share is not minute share. The 7m 24s figure is the whole `codegg` `cfg(test)` compile from a prior hosted run, not a TUI share, and this planning turn did not re-measure it.

## 4. Invariants that must not regress

- The root library does not depend on the TUI crate, under any feature, including dev-dependencies and build-dependencies.
- `src/lib.rs` does not declare `pub mod tui` and does not `pub use` the TUI crate.
- No shim module re-exports the TUI under the old path.
- The `codegg` bin is not a target of the root package. Workspace `cargo build --locked --bin codegg` still produces `target/debug/codegg`. The installed name stays `codegg`.
- Frontends still render projections. This milestone does not give the TUI crate provider credentials, project authorization, the command catalog, or durable session state.
- Attach still answers through `App::is_ws_attach_transport()`. The transport-authority guard follows the file and still fails if a mode-only gate is injected.
- `SemanticTheme` remains the service-side theme type and does not reference ratatui. Bundled theme files under `assets/themes/` are unchanged.
- Security-review abort still cancels the in-flight review. The task identity moves with `SecurityReviewTaskState`; the receipt module stops naming `TuiTaskId`.
- Every test function that existed under the moved surface still exists. Deltas are renames or moves, attributed. None is deleted by milestone name.
- The hosted clippy command stays `cargo clippy --workspace --all-targets`. The hosted nextest command stays `cargo nextest run --workspace --profile ci` with no extra features. No family is removed from either.
- Writer-fence, hostile-Git, queue-transaction, and sandbox fail-closed coverage are not edited to go green.
- `interactive_process_attach_resume`, `scheduler_cancellation`, and `causal_observe_replay` stay exclusive. Comment-budget ceilings are not raised.

## 5. Scope

### In scope

- Crate `codegg-tui` containing the current `src/tui` tree, the ratatui projection, and `SecurityReviewTaskState`.
- A thin bin package for `codegg` (ADR-0064's `codegg-cli`, unless §14 stops that and the bin must live in the TUI package instead). `src/main.rs` and `src/client/attach.rs` move there.
- Moving test-only reverse edges and the `tui_render` family to a package that may depend on both.
- A dependency-direction guard with `--self-test`, in `quick` and CI.
- A `rust_test_scan.py` before/after count with zero unexplained loss.
- Path updates in guards that read `src/main.rs`, `src/client/attach.rs`, or `src/tui/**`.
- `docs/test-ownership.toml` updates for moved paths.
- The minimum `AGENTS.md` and testing-skill corrections that a moved `--test tui_render` target makes false, taken from the baseline text.

### Explicitly out of scope

- Extracting `agent`, `tool`, `core`, `scheduler`, or `server`.
- A root `pub mod tui` or `pub use` compatibility shim.
- A root dev-dependency on the TUI crate.
- Changing CLI flags, doctor output, headless startup, daemon bootstrap, or attach wire behavior.
- Changing keymap, render bytes, or bundled theme files.
- Desktop theme adoption beyond leaving `SemanticTheme` in the service.
- Separate daemon/TUI deployment packaging, or a headless product binary as a new topology.
- Dropping `tui_render` or any other family from the hosted nextest command.
- `scripts/verify.sh scoped` as the iteration loop. `iter` as a gate. `sccache`, higher nextest `ci` threads, `--all-features`, or a CI split that recompiles the workspace per lane.
- The sandbox-helper cut, except that this milestone must not put the helper bin back in the root or add a TUI dependency to the helper.
- Quoting 236,317 lines as a fraction of the 7-minute compile.

## 6. Required production changes

### Core/domain

No domain-type change. Schedule-label helpers used only by `src/core/daemon.rs` tests stay where they are if the tests move to a package that can see both. If a production core path is found calling them, stop. The baseline evidence says that import is test-only.

### Storage and migrations

None.

### Protocol and DTOs

No protocol change. `crate::protocol::tui::TuiMessage` is the protocol module, not the UI crate. Do not move it. Attach may keep using it from the protocol crate.

### Runtime and concurrency

No scheduler change. Security-review task abort keeps the same cancellation behavior. The struct moves to the TUI crate; the service receipt stops storing `TuiTaskId`. Do not invent a service-side copy of `TuiTaskId` to avoid moving the struct. That leaves a UI identity in the service.

### Frontend or operator surface

The product bin moves. User-facing behavior does not. `cargo build --bin codegg` from the workspace still works because the bin name stays unique. The root package must not also have `src/main.rs`, or the bin name collides.

`codegg-lsp-test-server` stays a root bin if it does not reference the TUI. Do not move it "for consistency".

### Security and authorization

No authorization change. The TUI crate must not gain a path that answers a permission, steers an agent, or mutates a workspace except by the daemon calls it already makes. The transport-authority guard stays. Sandbox and writer-fence code are not part of this move.

### Documentation and static guards

Guards follow files. The new direction guard joins `quick` and CI. Ownership toml is updated. Architecture notes that say the TUI is a module of the root library are corrected to the one-way dependency. They do not restate ADR-0064 in full.

## 7. Ordered work packages

### Work package A — Count the tests before moving them

Intent:

Make unexplained test loss detectable.

Required changes:

At the baseline, before the move, run `scripts/rust_test_scan.py` over `src/tui`, `tests/tui_render`, and the test modules named in §3. Record the function count and the function paths in the milestone notes or in a fixture the later guard reads. Do not count with a regex. A regex scan misreads Rust and is how this repo already undercounted `tests/`.

Acceptance evidence:

The recorded count is in the commit that precedes the move, or in the closure with the baseline SHA and the scanner command. A count taken only after the move does not count.

### Work package B — Break the production reverse edges

Intent:

Remove every production reason for the root library to name the TUI, before the crate split. Doing it after the split forces a temporary cycle. Doing it by shim cancels the win.

Required changes:

- Move the ratatui projection (`src/theme/target.rs` and the `Theme` import in the registry) into the code that will become the TUI crate. Registry and `SemanticTheme` stay service-side and return `SemanticTheme`. The TUI projects it. Bundled theme file bytes stay identical.
- Move `SecurityReviewTaskState` into the TUI tree. Delete the `TuiTaskId` field from the service receipt. Update the two TUI call sites. Do not add a parallel service id type.
- Move `src/client/attach.rs` to the future bin package. `src/server` stays. The root library's `server` feature must compile without the TUI crate. If another file under `src/client` imports the TUI, it moves with attach. If `src/server` imports the TUI, stop. The baseline grep found none.

Acceptance evidence:

A grep of the root library source, excluding the yet-to-move `src/tui` and excluding `#[cfg(test)]`, finds no `crate::tui` and no `codegg_tui`. `SemanticTheme`'s module does not reference `ratatui`.

### Work package C — Extract the TUI library

Intent:

`src/tui` becomes `crates/codegg-tui`, depending on the root library. The root library does not depend on it.

Required changes:

Add the workspace member. Move the tree. Replace internal `crate::tui::` paths with the crate's own paths. Delete `pub mod tui` from `src/lib.rs`. Do not add a `pub use`. The TUI crate depends on the root library with that library's default features, not `--all-features`.

`src/theme` stays in the root. The TUI crate calls the service theme API and owns the ratatui projection.

Acceptance evidence:

`cargo metadata` shows `codegg-tui` depends on `codegg` and `codegg` does not depend on `codegg-tui`. `cargo check -p codegg --lib` succeeds and does not build `codegg-tui`. The closure shows that package list from the build log.

### Work package D — Move the product bin

Intent:

Keep the one-way edge. A root `src/main.rs` that calls the TUI cannot exist without a cycle.

Required changes:

Create the thin bin package. Move `src/main.rs` and the attach entry into it. The bin name is `codegg`. Delete the root `src/main.rs` so the implicit bin does not collide. The package depends on the root library and on `codegg-tui`. It does not become a library the root depends on.

Retarget every guard that reads `src/main.rs` or `src/client/attach.rs` to the new path in the same change. Known readers include `check_daemon_cwd_usage.py`, `check_scheduler_bypass.py`, `check_headless_workspace_registry_binding.py`, `check_request_ledger_writers.py`, `check_config_provenance.py`, `check_no_nested_runtime_block_on.py`, `check_scratch_reclaim_invariants.py`, and the TUI transport-authority guard. Grep rather than trusting this list. A guard whose check cannot be retargeted without dropping an assertion is a stop.

If the thin package cannot host `src/main.rs` without changing doctor, headless, or daemon startup behavior, stop. ADR-0064 permits putting the bin in the TUI package only in that case. It does not permit leaving the bin in the root.

Acceptance evidence:

`cargo build --locked --bin codegg` from the workspace writes `target/debug/codegg`. `cargo build -p codegg --bin codegg` fails because the root no longer has that bin. That failure is intended. Callers in release scripts are updated in the same change. The installed artifact name is unchanged.

### Work package E — Move the tests that bind both

Intent:

Preserve coverage without a cyclic dev-dependency.

Required changes:

Move `tests/tui_render` and the test modules named in §3 to the TUI package's tests, or to a test package that depends on both and is not the root package. Keep the Cargo target name `tui_render` if a workspace `cargo test --test tui_render` invocation is documented. If the target must be package-qualified, update the documented command. Do not drop the family from the hosted nextest command. The hosted command is `--workspace`, so a moved member is still selected. Prove that with a nextest list, not with an assumption.

Update `docs/test-ownership.toml` in the same change. A row that still points at a deleted path is a failed move.

Compare the scanner count from work package A. Every missing function is a recorded rename or an explained deletion. An unexplained deletion fails the milestone. Do not delete a test because its module path changed.

Acceptance evidence:

Scanner before/after, with the delta list. `cargo nextest list --workspace --profile ci` still contains the moved tests. Paste the list filter, not a claim that workspace mode "should" see them.

### Work package F — Transition guard

Intent:

Reject the re-export and the reverse dependency that cancel the compile win.

Required changes:

Add a guard, with `--self-test`, that fails when:

- package `codegg` depends on the TUI package, including optional, dev, and build dependencies, under any feature;
- `src/lib.rs` contains `pub mod tui` or a `pub use` of the TUI crate;
- a root-library source file outside `#[cfg(test)]` references the TUI crate;
- both the root package and another package define bin `codegg`;
- the root package still has `src/main.rs`.

`--self-test` injects a `pub use` and a dependency line and proves the guard rejects each. Wire it into `run_guards()` and into `.github/workflows/ci.yml`. Both, not one.

Acceptance evidence:

`--self-test` output, and both wiring sites in the diff.

## 8. Failure, cancellation, restart, and contention semantics

No new failure mode is introduced. Security-review cancellation still aborts the in-flight review by the moved task id. A move that compiles but drops the abort call is a behavior regression, not a packaging detail. The existing abort test moves and still fails if the call is gone.

Attach still uses the transport-authority predicate. A mode-only answer path remains the bug the existing guard rejects.

A half-moved tree, with `pub mod tui` left as a shim so that old paths compile, is a failed milestone. Remove the shim. Do not document it as temporary.

No new lock, lease, or restart protocol.

## 9. Compatibility and migration

No storage migration. No protocol negotiation. No config fallback.

The compatibility constraint is the compile graph and the installed binary name:

- `codegg::tui` is not a supported path after this milestone. In-tree callers move. An external crate depending on `codegg::tui` is not a supported public API this plan preserves; the root library is not published as that facade. Do not add a re-export to be kind to a hypothetical out-of-tree user.
- `cargo build --bin codegg` and `cargo build --bin codegg-sandbox-helper` from the workspace keep those bin names.
- `cargo build -p codegg --bin codegg` stops working. Update in-repo callers. Do not restore the root bin.
- Bundled theme bytes are unchanged, so existing user theme files keep parsing through the same `SemanticTheme` schema.

## 10. Required tests

### Focused unit tests

- Scanner count preservation from work packages A and E.
- A projection test: for each bundled theme, the TUI's ratatui `Theme` matches the pre-move projection. Capture that expected projection at the baseline, before the projection moves, the same way milestone 001 freezes status bytes. Do not regenerate it from the moved projection.
- A compile-fail or guard assertion that the `SemanticTheme` module does not reference `ratatui` or `tui::theme::Theme`.
- The existing security-review abort test, moved, still aborting the same way.

### Integration tests

- The `tui_render` family, whole, not one member standing in for it. `cargo test --test tui_render` without a trailing filter runs the family; after the move, the package-qualified equivalent must run the same members. List them.
- Daemon-socket tests that constructed `App`, moved and still asserting the subscription and task-row behavior they asserted before.
- Do not drop `interactive_process_attach_resume` coverage if a moved file is in its ownership set. Run that target if ownership says so. Do not un-exclusive it.

### Restart and recovery tests

None new. Attach reconnect behavior is whatever the existing attach tests already cover. Move those tests. Do not add a new reconnect protocol.

### Contention and cancellation tests

The security-review abort test is the cancellation test for this move. Scheduler cancellation tests are not weakened if a path guard's edit touches a file they scan.

### Security and negative tests

- Transition guard `--self-test` rejects an injected reverse dependency and an injected `pub use`.
- Transport-authority guard still rejects an injected mode-only answer path, at the attach file's new path.
- `cargo check -p codegg --features server` succeeds and the transition guard still reports no TUI dependency. Do not pass `--all-features`. The `verify.sh full` feature set (`server,plugins,lsp-test-support`) is the wider local check if `quick` is green and the change touched feature-gated attach. It is not a license to enable `lsp-real-server-tests`.

### Migration and compatibility tests

- The frozen theme-projection fixture.
- `cargo build --locked --bin codegg` produces the workspace target artifact.
- A negative check that `src/lib.rs` has no `pub mod tui`. The transition guard is that check. Do not also keep a shim "just for one release".

## 11. Required verification commands

Narrow tests first. Then the new guard. Then `quick`. Not `scoped`. Not `iter` as a gate. Not `--all-features`.

```bash
python3 scripts/rust_test_scan.py --validate

# package-qualified after the move; the family filter stays the family, not one test
cargo test -p codegg-tui --test tui_render -- tui_render
cargo test -p codegg --lib --features server

python3 scripts/check_tui_dependency_direction.py --self-test
python3 scripts/check_tui_transport_authority.py

cargo build --locked --bin codegg
scripts/verify.sh quick
```

The guard and package names may differ. The `--self-test`, the family-level TUI run, the `server` feature check of the root library, and `quick` may not.

If `cargo test --test tui_render` must become package-qualified, update the documented command in the same change. Do not leave `AGENTS.md` naming a target that no longer resolves. Do not stage the unrelated drafts in that file.

Hosted closure, not a local substitute:

- before and after run ids for the `codegg` `cfg(test)` compile on the hosted `CI / verify` job;
- wall clock reported with the runner note the closed throughput roadmap already requires: host-speed difference is the same order as a small effect, so one pair of runs is not a precise effect size;
- a statement of whether the root unit still contains the TUI (it must not) and whether the workspace job still builds the TUI crate (it must).

A null improvement closes this milestone if the invariants hold. It does not authorize extracting `agent`, `tool`, `core`, or `scheduler`.

## 12. Documentation updates

- Architecture notes that call the TUI a root module, corrected to the one-way dependency and the bin package. No fragile line numbers.
- `docs/test-ownership.toml`.
- `AGENTS.md` and `.opencode/skills/testing-ci/SKILL.md` only where a command or path this move breaks is stated. Baseline text only. The skills index row for `testing-ci` stays; this plan does not add a skill.
- Guard headers that name `src/main.rs` as the CLI bootstrap.
- Do not amend the closed throughput roadmap to claim this split as its result.

## 13. Acceptance criteria

- `cargo check -p codegg --lib` does not build the TUI crate. `cargo metadata` shows no reverse dependency under any feature.
- `src/lib.rs` has no `pub mod tui` and no TUI re-export. No shim file restores `codegg::tui`.
- `cargo build --locked --bin codegg` produces `target/debug/codegg`. The root package has no `src/main.rs`.
- Bundled theme bytes are unchanged. The frozen projection fixture matches. `SemanticTheme` does not reference ratatui.
- Security-review abort and attach transport authority still pass their existing tests at the new paths.
- Scanner count has zero unexplained loss. `tui_render` is still selected by the workspace nextest command.
- The transition guard is in `quick` and CI, and `--self-test` rejects an injected `pub use` and an injected reverse dependency.
- The hosted clippy and nextest command lines are unchanged, aside from a guard script this milestone adds to the workflow's guard list.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- ADR-0064 is still proposed.
- A production `crate::tui` edge exists outside the set in §3. Do not "just move it" if it means the service library must keep a UI type. Report the edge.
- `src/server` imports the TUI. This plan does not extract the server.
- The thin bin package cannot host `src/main.rs` without a behavior change. Report that. Do not rewrite doctor or headless to force the move, and do not leave the bin in the root.
- A guard cannot follow `src/main.rs` or attach without losing a check.
- A test family cannot move without losing tests or leaving the hosted `--workspace` selection. Do not drop the family.
- The frozen theme projection cannot be preserved without changing bundled theme bytes or render output. Stop. A render change is out of scope.
- The only green path is a `pub use`, a root dev-dependency, or deleting tests. Stop.
- The hosted measurement is null and the temptation is to split `agent` or `tool` under this plan. Stop. That split is a different ADR.
- Unrelated dirty CI, `verify.sh`, or `AGENTS.md` drafts cannot be kept out of the commit. Stop.

## 15. Closure evidence required

- Baseline and move commits, plus the pre-move scanner count and theme-projection fixture.
- `cargo metadata` excerpt showing the edge direction, and a `cargo check -p codegg --lib` log showing the TUI crate was not built.
- Guard `--self-test` output and the `quick` plus CI wiring diff.
- §11 commands with pass/fail, labeled local. Hosted before/after run ids, labeled hosted, with the null-or-not result stated in words. No line count presented as a minute count.
- Ownership-toml diff and the attributed test-function delta. Zero unexplained loss.
- A sentence that the closed throughput roadmap was not amended and that a null result did not open a further split.

## 16. Handoff notes

This plan is proposed. Do not mark it `active` in the registry until ADR-0064 is accepted and implementation starts. Milestone 001 may land first or not at all; do not wait for it, and do not fold the helper bin into this change.

The compile win is the absence of the reverse edge, not the existence of the crate. A review that finds `pub mod tui` or a root dependency on `codegg-tui` rejects the change even if every test passes.

Start from a clean tree at the baseline. The dirty iter/junit drafts are a different effort and must not ride along.
