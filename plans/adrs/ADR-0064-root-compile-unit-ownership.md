# ADR-0064: Root Compile-Unit Ownership

Status: accepted — accepted 2026-11-21 by the project maintainer

Date: 2026-11-21

Decision owners: project maintainers

Accepted 2026-11-21 by the project maintainer. Acceptance authorizes the two milestone plans that cite this record. The decision text below is the accepted contract and is not rewritten by acceptance.

Related specification sections:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#44-frontends-render-projections`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Affected subsystem roadmaps:

- `plans/subsystems/root-compile-boundary-roadmap.md`

Related ADRs:

- ADR-0010 (accepted) — desktop frontend and shared client boundary. This record does not revisit it. The desktop stays a daemon client.
- ADR-0016 (accepted) — platform-owned sandbox launch and obtained-enforcement authority. This record does not change launch authority, obtained-enforcement reporting, or the private status stream.
- ADR-0046 (accepted) and ADR-0061 (accepted) — Linux isolation and the default Linux containment policy. This record does not promote a backend, change the default policy, or lift a `cfg(target_os = "linux")` gate.
- ADR-0004 (accepted) — approval routing and sandbox preferences. Profile and approval semantics stay as they are.
- The closed CI and test throughput recovery roadmap neither accepts nor rejects a root split. This record does not amend it and is not a continuation of it.

## Context

At planning baseline `b8a6215c885289654d5a8ee951bcccde401a529e`, the root package is one Cargo unit: 749 Rust files, 663,456 lines. `src/tui` is 236,317 of those lines. The hosted job compiles that unit with `cargo clippy --workspace --all-targets`, builds the helper with `cargo build --locked --bin codegg-sandbox-helper`, and tests with `cargo nextest run --workspace --profile ci`, with no extra features.

The helper source is 889 lines (`src/bin/codegg-sandbox-helper.rs`). It is a root-package bin and links the root library (`use codegg::security::sandbox`). The hosted helper step therefore codegens the root. Prior hosted observation, not re-measured in this planning turn: on warm run `44837892044` the helper step was 4m 10s, of which 3m 11s was root codegen. That observation is a reason to cut the bin out of the root package. It is not a promised saving, and it does not move the long `codegg` `cfg(test)` compile.

The cut that can shrink that long compile is a one-way TUI dependency. Production reverse edges at the baseline are:

- `src/theme/registry.rs` and `src/theme/target.rs` import `crate::tui::theme::Theme`. The theme module is otherwise consumed only by itself and `src/tui`. `SemanticTheme` is already the frontend-neutral type.
- `SecurityReviewTaskState` in `src/security/workflow/receipt.rs` stores `TuiTaskId`. Its only production users are the TUI app and the TUI security-review command.
- `src/client/attach.rs` imports `crate::tui`. The client module is compiled only with `feature = "server"`. `src/server` does not import `crate::tui`.
- `src/main.rs` is the implicit `codegg` bin and has three `crate::tui` references. Cargo dependencies are package-scoped, so a root bin that calls the TUI crate forces the root package to depend on that crate.

Test-only reverse edges, which must move rather than become a dev-dependency, include `src/core/daemon.rs` tests, `src/core/transport/daemon_socket_integration_tests.rs`, `src/security/workflow/mod.rs` tests (the `#[cfg(test)] mod tests` gate), and the root integration family `tests/tui_render`.

A `pub use` of an extracted TUI crate from `src/lib.rs` would put that crate back in the root unit and cancel the compile win. The existing `pub use codegg_core` pattern is right for a small library the root must call. It is the wrong pattern for the TUI.

`codegg-core` inherits `unsafe_code = "deny"` and must stay UI/server/plugin/auth-free. The helper's reviewed `unsafe` (status-pipe plumbing, `sandbox_init` FFI, Linux PTY ioctls, seccomp) cannot move into core. `open_pinned` already has a narrow `#[allow(unsafe_code)]` and is shared by the Landlock compiler and the helper.

Line share is not minute share. No hosted measurement of a split exists. Whether a smaller root rlib would fit the restored cache is a hypothesis, not a result.

## Decision drivers

- The service library and the helper bin must be compilable without compiling each other.
- The service library must not depend on a UI crate.
- Fail-closed sandbox behavior, the installation-sibling helper identity, and the private launch spec and status frame must not change.
- The hosted workspace clippy and nextest commands must not become a way to compile less product.
- A further mechanical split of `agent`, `tool`, `core`, or `scheduler` is not justified by the evidence at this baseline.

## Considered options

### Option A — Leave both inside the root package

No ownership change. Incremental compilation has no crate boundary to increment across. The helper step keeps compiling the root because the helper remains a root bin. Rejected.

### Option B — Extract agent, tool, core, and scheduler along the import graph

Production `agent` and `tool` calls are real. A syntactic strongly connected component that includes test edges is not a production indivisibility proof, and it is also not a clean seam. Rejected for this decision. A later decision may revisit it only with a new ADR and a hosted measurement that the TUI cut did not shrink the dominating unit.

### Option C — Fold the helper into `codegg-core`

Core denies `unsafe_code` and must not gain a UI, server, plugin, or auth dependency. The helper's reviewed `unsafe` does not belong there. Rejected.

### Option D — Feature-gate the TUI inside the root package

A feature does not create a second crate. The default unit, which the hosted job builds, would still contain the TUI when the feature is on, and would drop TUI coverage when it is off. Rejected.

### Option E — Re-export the extracted TUI from the root library

`pub use` of the TUI crate from `src/lib.rs` puts the extracted unit back in the root compile. That is the failure mode this decision exists to forbid. Rejected.

### Option F — One-way compile units

Selected, and recorded below. Two independent cuts, one ownership rule.

## Decision

1. The sandbox helper binary leaves the root package. The package that owns the bin named `codegg-sandbox-helper` must not depend on the root package `codegg`, as a normal, dev, or build dependency, under any feature. The root package must not keep that bin, including via `src/bin/codegg-sandbox-helper.rs` or a shim that re-exports the new bin.
2. The helper-facing contract moves to a workspace library the helper and the root may both depend on. The root may depend on that library and may re-export its types so existing `crate::security::sandbox` callers keep compiling. That re-export is allowed because it does not pull the helper bin, or the root library, into the helper's compile. The library may depend on `codegg-core`. It must not depend on the root package. Parent-only couplings stay in the root: installation-sibling path resolution (`crate::install`), `ToolError` mapping, and any test that needs `crate::tool`.
3. Reviewed `unsafe` stays in the helper bin. The sandbox library inherits the workspace `unsafe_code = "deny"` lint. Existing function-level allows, including `open_pinned`, stay narrow and stay on the function. They do not move into `codegg-core`, and they do not become a crate-wide allow. Linux-only modules stay `cfg(target_os = "linux")`.
4. The installed helper stem stays `codegg-sandbox-helper`. Resolution stays the installation-owned sibling. `PATH`, cwd, and `CODEGG_SANDBOX_HELPER` stay out of production identity. `SandboxLaunchSpec` JSON, the length-prefixed `SandboxStatusFrame`, `SANDBOX_STATUS_VERSION`, and the process-scope ABI record line stay byte-compatible. A version bump is a protocol change and is outside this decision.
5. The TUI leaves the root library. A new workspace library may depend on the root library. The root library must not depend on that TUI library, including through an optional feature, a dev-dependency, a build-dependency, or a `pub use` / `pub mod tui` re-export. `src/lib.rs` must not declare `pub mod tui`.
6. The product binary cannot stay a target of the root package. Cargo dependencies are package-scoped, and `src/main.rs` calls the TUI, so a root bin that keeps those calls would require the forbidden edge or a cycle. The `codegg` bin moves to a workspace member that may depend on both the root library and the TUI library. The bin name stays `codegg`. The workspace target artifact stays `target/<profile>/codegg`. This is a compile-unit move, not a deployment split: one installed `codegg` binary remains, and this decision does not authorize a headless product topology or a separate daemon/TUI distribution.
7. The preferred bin package is a thin member (named `codegg-cli` unless the implementation plan records a better name) so the TUI crate stays a library and does not become the owner of doctor, headless, and daemon bootstrap. Putting the bin in the TUI package is permitted only if the thin member cannot host `src/main.rs` without a behavior change. Leaving the bin in the root package is not permitted.
8. `SemanticTheme` stays the frontend-neutral theme type. The ratatui `Theme` projection moves to the TUI crate. `SecurityReviewTaskState` moves to the TUI crate; the service receipt type must not store `TuiTaskId`. `src/client/attach.rs` moves to a package that may depend on the TUI. The root library's `server` feature must still compile without a dependency on the TUI crate. `src/server` stays where it is; this decision does not extract the server to shorten the hosted job.
9. Tests that use both the service library and the TUI move to a package that may depend on both. They are not deleted, and they are not kept by a root dev-dependency on the TUI crate. Root integration families that bind the TUI, including `tui_render`, move with their tests. The hosted nextest command stays `cargo nextest run --workspace --profile ci` and keeps selecting them because they remain workspace tests.
10. Neither cut removes the TUI, the helper, or a test family from the hosted clippy or nextest commands. The helper step may gain a package selector. It keeps building the bin named `codegg-sandbox-helper` and keeps the copy into `target/debug/deps/` that integration tests load. Fail-closed containment, writer-fence, hostile-Git, and queue-transaction coverage stay.

A null hosted measurement is a successful closure of the boundary if the invariants hold and the measurement is recorded. It is not permission to continue into option B under this ADR.

## Consequences

### Positive

- The helper bin can be built without compiling the root library.
- The root library can be checked and unit-tested without compiling the TUI.
- The failure mode that cancels the TUI win (a root re-export or a reverse dependency) is a stated prohibition, not an implementation preference.
- Sandbox launch bytes and the installed binary names stay stable.

### Negative

- The product bin leaves the package developers currently pass to `-p codegg`. Callers, guards, and docs that assume `src/main.rs` lives in the root package must follow it in the same change.
- Two new workspace members, plus a thin bin member, increase the workspace surface. That cost is accepted. A shim module left behind to avoid it is not.
- The root unit loses the TUI's lines only after the reverse edges and the bin move land together. Landing the crate without removing the edges does not count.

### Neutral or deferred

- No hosted minute count is claimed. Line share is not minute share. Cache fit of a smaller rlib is unmeasured.
- Agent, tool, core, and scheduler stay in the root library.
- The server stays feature-gated in the root library, minus the attach entry that imports the TUI.
- The closed throughput roadmap's open feature-gated coverage decision (its §10 D1) is untouched.
- Separate daemon/TUI packaging as a product distribution remains the deferred unregistered item. This decision does not discharge it.

## Compatibility and migration

No storage migration. No public protocol change. The private helper status frame and launch spec are compatibility contracts for already-installed helpers and parents: old bytes must still decode, and new bytes must match the baseline encoder. Capture those bytes from the baseline encoder before moving the type. Do not regenerate the fixture from the moved encoder.

Installed names stay `codegg`, `codegg-sandbox-helper`, and `codegg-eggsearch`. Upgrade and installer membership rules that key off those stems stay. A release script that invokes `cargo build -p codegg --bin codegg` is updated in the same change to the package that owns the bin. The installed artifact name does not change.

`SANDBOX_STATUS_VERSION` is not bumped. `SandboxProfile`, `ApprovalMode`, and the default Linux policy are not reinterpreted.

## Security and reliability implications

The helper remains a one-shot child. It still takes one bounded launch description and one private status-pipe descriptor, applies the containment policy, reports setup on that pipe, and execs the target. It does not gain a daemon, a public protocol, or a PATH lookup.

Missing helper, truncated status, duplicate terminal status, and sandbox-unavailable constrained execution stay fail-closed. This decision does not add an uncontained fallback.

The reviewed `unsafe` does not move into the root library or into `codegg-core`. A package-level allow that covers the helper and the library together is a widening and is rejected. The library denies `unsafe_code`; the bin is outside that deny, which is why the bin and the library are split if a shared package lint cannot express that.

Writer-fence, hostile-Git, and queue-transaction behavior are not in this decision's change set. A cut that requires weakening one of those guards is a stop, not a tradeoff.

The TUI move does not move authorization into the frontend. Frontends still render projections. Attach still uses the existing transport authority rule (`is_ws_attach_transport`), enforced by the existing guard, whose path list follows the file.

## Verification

Conformance is the two milestone plans' transition guards and frozen-byte tests, plus the existing sandbox and TUI suites those plans name. A guard that is not wired into both `scripts/verify.sh` `run_guards()` and `.github/workflows/ci.yml`, or that has no `--self-test` injecting the reverse edge, does not count.

Closure also records a hosted before/after of the helper step and of the `codegg` `cfg(test)` compile, with run ids. One run is not a precise effect size. A closure that reports no improvement and does not expand scope satisfies this decision. A closure that quotes line counts as minutes does not.

## Supersession

None. This record does not supersede ADR-0016, ADR-0046, ADR-0061, or ADR-0010.
