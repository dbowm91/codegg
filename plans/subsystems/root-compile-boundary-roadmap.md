# Root Compile Boundary Roadmap

Status: closed

Long-term references:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#44-frontends-render-projections`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Related ADRs:

- `plans/adrs/ADR-0064-root-compile-unit-ownership.md` (accepted 2026-11-21; gates both milestones)
- ADR-0016, ADR-0046, and ADR-0061 are binding constraints, not work items. This roadmap does not amend them.

Repository planning baseline: `b8a6215c885289654d5a8ee951bcccde401a529e`

Primary class: infrastructure.

This roadmap is not a continuation of the closed `ci-test-throughput-recovery` roadmap. That roadmap neither accepts nor rejects a root split. ADR-0064 was accepted on 2026-11-21. Both milestones are closed. The hosted compile pair is recorded and is not a precise effect size. That record does not open a further split. The deferred binary-topology item is not this roadmap's work and is not discharged.

## 1. Purpose and ownership boundary

This workstream owns two compile-unit boundaries in the root package, and the regression tests that prove the transition did not change behavior.

It owns:

- which package owns the `codegg-sandbox-helper` bin;
- which package owns the helper-facing launch contract;
- which package owns the TUI, and the direction of its dependency on the service library;
- which package owns the `codegg` product bin after that split;
- the static guards that reject a reverse edge or a re-export that puts an extracted unit back in the root.

It consumes, and must not redefine:

- sandbox launch authority and obtained enforcement (ADR-0016);
- Linux isolation and the default Linux policy (ADR-0046, ADR-0061);
- the frontend rule that TUIs render projections and do not own credentials, authorization, scheduling, or durable session state;
- the daemon-owned command catalog;
- writer-fence, hostile-Git, and queue-transaction contracts.

It must not own a new sandbox framework, a new frontend, or a deployment topology.

## 2. Work classification

### Invariants

- The package that owns the `codegg-sandbox-helper` bin does not depend on the root package.
- The root library does not depend on the TUI crate, under any feature, including a dev-dependency or a `pub use` re-export.
- The private launch spec and status frame stay byte-compatible with the baseline encoder. `SANDBOX_STATUS_VERSION` is not bumped.
- Helper identity stays the installation-owned sibling stem `codegg-sandbox-helper`. Production resolution does not consult `PATH`, cwd, or `CODEGG_SANDBOX_HELPER`.
- Constrained execution with no helper, or with a status stream that fails the existing decoder, stays fail-closed. It does not spawn uncontained.
- No behavioral assertion is removed to make the move cheaper. Test functions move; they are not deleted by milestone name.
- The hosted workspace clippy and nextest commands do not lose a family, a bin, or the TUI.

### Capabilities

None. A faster edit-compile loop is not a user-facing capability. This roadmap must not be closed as one.

### Infrastructure

- A sandbox library plus a helper bin package, with the dependency direction in ADR-0064.
- A TUI library that depends on the service library, and a product bin package that may depend on both.
- Transition guards with `--self-test`, wired into `scripts/verify.sh` `run_guards()` and CI.
- Frozen status-frame and launch-spec fixtures captured from the baseline encoder before the type moves.

### Polish

Hosted compile-time measurement. It is closure evidence, not an acceptance threshold invented in advance. Line share is not minute share. A null result is reported and does not expand the roadmap.

## 3. Non-goals

- No product behavior change, no new command, no changed CLI flag, no changed installed binary name.
- No new sandbox framework, no network isolation by default, no promotion of the Linux production backend, no `SandboxProfile` or `ApprovalMode` change, no Windows sandbox work. Those non-goals already belong to the sandbox containment audit roadmap; this roadmap does not reopen them.
- No folding of the helper or its `unsafe` into `codegg-core`. Core stays UI/server/plugin/auth-free and keeps `unsafe_code = "deny"`.
- No mechanical split of `agent`, `tool`, `core`, or `scheduler`. Production `agent` / `tool` calls are real. A null TUI measurement does not authorize that split under this roadmap.
- No extraction of `server` to shorten this hosted job. The attach entry moves only because it imports the TUI.
- No `pub use` of the TUI crate from the root library. No shim `src/bin/codegg-sandbox-helper.rs` and no shim `pub mod tui`.
- No dropping of an integration family, and no deletion of tests by milestone name. A family leaves the hosted nextest command only after a red run names the failing test, which this roadmap does not schedule.
- No use of `scripts/verify.sh scoped` as the iteration loop. `iter` is not a gate and is not retargeted at hosted CI.
- No `sccache`, no `CARGO_PROFILE_TEST_DEBUG=0`, no raising of nextest `ci` threads above 4, no `--all-features`, and no CI split that makes each lane recompile the workspace.
- No un-exclusivity of `interactive_process_attach_resume`, `scheduler_cancellation`, or `causal_observe_replay`. No raising of ceilings in `docs/comment-budget.toml`.
- No separate daemon/TUI deployment packaging, and no headless product topology. The deferred registry item stays deferred.
- No amendment of pushed commits. The unrelated unstaged iter/junit drafts in the planning worktree are not part of the baseline and are not this roadmap's files.
- No quoting of the 7m 24s `cfg(test)` compile, or of the helper step's 3m 11s root codegen, as a promised saving. Those figures are prior hosted observations. This roadmap's closure measures again.

## 4. Current state

At `b8a6215c885289654d5a8ee951bcccde401a529e` the root package is one library and an implicit `codegg` bin (`src/main.rs`), plus two explicit bins: `codegg-lsp-test-server` (feature `lsp-test-support`) and `codegg-sandbox-helper`.

Recounted at planning time, not taken from the earlier research note:

- root `src/**/*.rs`: 749 files, 663,456 lines;
- `src/tui`: 236,317 lines;
- `src/theme`: 3,999 lines, consumed by itself and `src/tui` only;
- `src/bin/codegg-sandbox-helper.rs`: 889 lines;
- `src/security/sandbox.rs`: 1,761 lines;
- `src/security/sandbox/`: 5,759 lines.

The helper imports the root library. Building the bin therefore compiles the root. The sandbox module's production imports of the rest of the root are narrow: `ToolError`, installation-sibling path resolution, and one `managed_process` helper that derives mount-view limits from `SandboxLaunchSpec`. A `crate::tool` use in the sandbox policy module is inside a test. The helper-facing surface can leave the root. The parent-only path resolution stays.

Production TUI reverse edges are the theme projection, `SecurityReviewTaskState`'s `TuiTaskId`, `src/client/attach.rs` under the `server` feature, and the product bin. `src/server` does not import the TUI. `src/agent`, `src/tool`, and `src/scheduler` do not import the TUI. Core and security-workflow imports of the TUI that were checked are inside `#[cfg(test)]`.

The hosted commands at that baseline are `cargo clippy --workspace --all-targets`, `cargo build --locked --bin codegg-sandbox-helper`, and `cargo nextest run --workspace --profile ci`, with no extra features. The helper step copies the bin into `target/debug/deps/` because constrained integration tests load it from there. Product builds still resolve only the installation-owned sibling.

Prior hosted observation, from run `44837892044` on `827141e32`, not re-downloaded for this plan: the helper step spent most of its time codegenning the root. The 7m 24s figure is the whole `codegg` `cfg(test)` compile, not a per-module share. No split has been measured.

Guards that hardcode `src/bin/codegg-sandbox-helper.rs` include the sandbox contract guard and the sandbox policy-wiring guard. Several guards read `src/main.rs` as the CLI bootstrap. Those path lists follow the files. They are not weakened, and a leftover path is not satisfied by a shim.

## 5. Target architecture

```text
codegg (root library, service)
    depends on codegg-sandbox (library only)
    does not depend on codegg-tui
    does not own bin codegg
    does not own bin codegg-sandbox-helper

codegg-sandbox (library)
    launch spec, status frame, apply/encode/decode, open_pinned
    depends on codegg-core as needed
    does not depend on the root package
    denies unsafe_code, with the existing function-level allows

codegg-sandbox-helper (bin package)
    the reviewed unsafe
    depends on codegg-sandbox
    does not depend on the root package
    bin name remains codegg-sandbox-helper

codegg-tui (library)
    src/tui, the ratatui theme projection, SecurityReviewTaskState
    depends on the root library
    does not depend on codegg-cli

codegg-cli (bin package)
    src/main.rs and the attach entry
    depends on the root library and on codegg-tui
    bin name remains codegg
```

The root library may re-export sandbox-library types. It may not re-export the TUI. The product bin and the helper bin are absent from the root package, so `cargo build -p codegg --lib` does not compile either bin's crate as part of the root unit.

Workspace `target/<profile>/` still receives `codegg` and `codegg-sandbox-helper`. Install and upgrade still key off those stems.

## 6. Dependency graph

```text
ADR-0064 accepted
    |
    +--> Milestone 1  sandbox-helper package cut
    |
    `--> Milestone 2  TUI dependency inversion
```

ADR-0064 acceptance is an operational dependency of both milestones. Neither milestone has a hard dependency on the other. Milestone 1 is the smaller cut and establishes the transition-guard pattern; Milestone 2 may still be handed off first after the ADR is accepted. Neither is ready while the ADR is proposed.

Interface dependencies already stable: ADR-0016 launch contract, the installation-sibling rule, the frontend projection rule, the workspace lint that denies `unsafe_code` in members that inherit it.

Soft: hosted measurement of Milestone 1 does not gate Milestone 2. A Milestone 2 closure that records a null compile effect does not open a third milestone on this roadmap.

## 7. Milestones

### Milestone 1 — Sandbox-helper package cut

Class: infrastructure

Objective:

Move the helper bin out of the root package so the hosted helper build no longer compiles the root library, without changing launch bytes, helper identity, or fail-closed containment.

Dependencies:

ADR-0064 accepted. No other hard dependency.

Deliverable boundary:

The bin named `codegg-sandbox-helper` is a workspace member whose dependency closure does not include package `codegg`. The helper-facing contract lives in a library both sides may use. Existing callers of `crate::security::sandbox` still compile. Frozen baseline bytes still decode. Transition guard is in `quick` and CI.

User or operator value:

None directly. The operator-visible helper name, install layout, and failure wording stay.

Exit conditions:

- The dependency-direction guard fails its `--self-test` when a root dependency or a root bin shim is injected, and passes on the tree.
- Baseline status-frame, launch-spec, and process-scope ABI fixtures match the moved encoder byte for byte.
- Existing sandbox contract and policy-wiring guards pass against the new paths, and still reject a direct `apply_landlock` dispatch and a PATH lookup.
- Containment, writer-fence, hostile-Git, and queue-transaction suites named in the implementation plan still pass. None was removed.
- The hosted helper step builds the new package's bin and still copies it to `target/debug/deps/`. Closure records whether that step still compiles package `codegg`. It must not.

Deferred work:

Hosted minute claims. Any parent-side refactor that is not required to keep the helper's dependency closure free of the root.

### Milestone 2 — TUI dependency inversion

Class: infrastructure

Objective:

Make the TUI a library that depends on the service library, and move the product bin to a package that may depend on both, so the root library no longer compiles the TUI.

Dependencies:

ADR-0064 accepted. No hard dependency on Milestone 1.

Deliverable boundary:

`src/lib.rs` has no `pub mod tui` and no TUI re-export. Cargo metadata shows no root dependency on the TUI crate under any feature. The `codegg` bin is not a root-package target. Moved tests are still present, counted by `scripts/rust_test_scan.py`, with every delta attributed. The hosted nextest command is unchanged and still selects the moved families.

User or operator value:

None directly. CLI behavior, attach transport authority, theme rendering, and security-review abort stay.

Exit conditions:

- The dependency-direction guard fails its `--self-test` on an injected `pub use` or reverse dependency, and passes on the tree.
- Test-function count under the moved TUI surface has zero unexplained loss.
- Bundled theme projection matches the pre-move projection. `SemanticTheme` does not reference ratatui.
- `cargo check -p codegg --features server` does not depend on the TUI crate. Attach still builds in the package that owns it.
- `cargo build --locked --bin codegg` from the workspace still produces `target/debug/codegg`.
- Closure records a hosted before/after of the `codegg` `cfg(test)` compile. A null result closes the boundary and does not open a further split.

Deferred work:

Extracting `agent`, `tool`, `core`, or `scheduler`. Extracting `server`. Desktop reuse of the theme registry beyond the `SemanticTheme` type this milestone leaves in the service. Separate daemon/TUI packaging.

## 8. Cross-cutting requirements

### Storage and migration

None. No schema change. No migration.

### Protocol and compatibility

No public protocol change. The private helper status frame is a compatibility contract, frozen before the move. `SANDBOX_STATUS_VERSION` stays. The process-scope ABI record key stays `codegg-landlock-process-scope-abi`.

### Security and authorization

Fail-closed containment stays. Reviewed `unsafe` stays in the helper bin. `open_pinned` keeps a function-level allow in the sandbox library. Authorization is not moved into the TUI. The attach path keeps the existing transport-authority guard.

### Concurrency, cancellation, and recovery

No scheduler or cancellation change. Security-review abort still cancels by the same task identity; only the type's owning crate changes. Helper launch stays one-shot. A partial package move that leaves the parent resolving the helper through `PATH` is a failed migration, not a fallback.

### Observability and audit

No new audit event. Closure records hosted run ids and local commands separately, and does not label a local timing as CI.

### Performance and resource use

The only performance claim this roadmap allows is a hosted measurement reported with dispersion, or an explicit statement that the split did not improve the measured step. Cache size of the root rlib is reported if measured. It is not a target.

### Documentation and static guards

Path-pinned guards follow moved files in the same change. New transition guards join both `quick` and CI and carry `--self-test`. `docs/test-ownership.toml` is updated when tests move. `AGENTS.md` and the testing skill are updated only where a command they name becomes false, and not by staging unrelated drafts.

## 9. Verification strategy

Each milestone's implementation plan names the commands. The roadmap requires:

- transition guards with `--self-test`, in `scripts/verify.sh` `run_guards()` and in `.github/workflows/ci.yml`;
- frozen-byte fixtures for the helper contract, captured before the move;
- a `rust_test_scan.py` count for the TUI move, not a regex count;
- the existing sandbox and TUI suites, not a reduced substitute;
- `scripts/verify.sh quick` as the local gate. Not `scoped`. Not `iter`;
- hosted measurement recorded in the closure, labeled as hosted, and not treated as a precise effect size from one run.

Equating compilation with closure is a planning anti-pattern. A green `cargo check` without the transition guard and the preserved tests does not close either milestone.

## 10. Risks and decision points

- ADR-0064 was accepted on 2026-11-21. That acceptance is the handoff gate; it is not permission to widen either milestone.
- The helper-facing surface may not separate cleanly from `managed_process`'s mount-view limit helper. The implementation plan's stop condition applies: invert the call or move that pure derivation with the spec. Do not depend on the root to finish the move.
- Moving `src/main.rs` may break guards that treat that path as the CLI bootstrap. Follow the file. If a guard's check cannot be retargeted without weakening it, stop.
- Root integration tests cannot take a dev-dependency on a TUI crate that depends on the root. They move. If a family cannot move without losing tests, stop. Do not drop it from the hosted command to get green.
- A dirty planning worktree contains unrelated edits to CI, `verify.sh`, `AGENTS.md`, and the testing skill. Implementers start from the baseline or isolate their edits. They do not stage those drafts.
- The compile win can be cancelled by a compatibility shim. The transition guards are the mitigation, and a milestone that lands without them is not done.

## 11. Completion definition

The roadmap is complete when both milestones have accepted closure records, ADR-0064's verification section is satisfied, and the registry says so. It is also complete if a closure records a null compile effect and explicitly does not open a further split. It is not complete when only the crates exist, when a shim preserves the old path, or when a local check is described as hosted CI.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/root-compile-boundary/001-sandbox-helper-package-cut.md` | `plans/closure/root-compile-boundary/001-status.md` | hosted helper step `44937196668` did not compile package `codegg` |
| 2 | closed | `plans/implementation/root-compile-boundary/002-tui-dependency-inversion.md` | `plans/closure/root-compile-boundary/002-status.md` | hosted retarget run `44940955257` passed `lsp::callHierarchy_requires_file_path_line_column` |
