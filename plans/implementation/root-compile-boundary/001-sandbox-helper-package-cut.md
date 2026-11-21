# Root Compile Boundary Milestone 001 — Sandbox-helper package cut

Status: implemented — conditionally closed pending a hosted helper-step run id. A local helper build on `a6fa12efb` did not compile package `codegg`; that is not the hosted run. See `plans/closure/root-compile-boundary/001-status.md`. ADR-0064 accepted 2026-11-21 by the project maintainer

Repository baseline: `b8a6215c885289654d5a8ee951bcccde401a529e`

The planning worktree also has unrelated unstaged edits (`.config/nextest.toml`, `.github/workflows/ci.yml`, `.opencode/skills/testing-ci/SKILL.md`, `AGENTS.md`, `architecture/testing.md`, `scripts/verify.sh`) and untracked `scripts/annotate_nextest_junit.py` and `scripts/iter_selection.py`. Those drafts are not the baseline. Do not stage them. If an edit this milestone must make overlaps a dirty file, restore that file to the baseline or stop. Do not merge the drafts in.

Source roadmap:

- `plans/subsystems/root-compile-boundary-roadmap.md#milestone-1--sandbox-helper-package-cut`

Long-term requirements:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs:

- `plans/adrs/ADR-0064-root-compile-unit-ownership.md` (accepted 2026-11-21)
- ADR-0016 (accepted) — launch authority and obtained enforcement; do not reinterpret
- ADR-0046 and ADR-0061 (accepted) — Linux isolation and default policy; do not amend

Primary class: infrastructure

## 1. Objective

Move the `codegg-sandbox-helper` bin out of the root package so that building that bin no longer compiles the root library, while the private launch spec, status frame, installed stem, and fail-closed containment stay byte- and behavior-compatible with the baseline.

## 2. Why this milestone is ready

ADR-0064 was accepted on 2026-11-21. That was the operational dependency, and it is satisfied. There is no other hard dependency. The launch contract and the installation-sibling rule are stable interface dependencies and must not be redesigned while moving the package.

## 3. Current implementation evidence

At the baseline the helper is a root-package bin at `src/bin/codegg-sandbox-helper.rs` (889 lines). Its module comment is the contract: one bounded JSON launch description, one private status-pipe descriptor, apply containment, report setup on that pipe, exec the target. It imports `apply_backend`, `encode_sandbox_status`, `platform_sandbox_capability`, `SandboxCapability`, `SandboxLaunchOutcome`, `SandboxLaunchSpec`, and `MAX_SANDBOX_SPEC_BYTES` from `codegg::security::sandbox`. It also calls `open_pinned`. Building the bin therefore compiles the root library. That is the whole of the helper-step problem. The helper source itself is not 8,400 lines. The sandbox module plus the helper is about 8,400 lines (1,761 + 5,759 + 889). The parent still needs the library; the win is that the helper package stops compiling the other 660,000 lines.

The root package does not inherit workspace lints because this bin has reviewed `unsafe` and `src/lib.rs` has `#![deny(unsafe_code)]`. The unsafe in the bin is status-pipe plumbing, the `sandbox_init` FFI, Linux PTY ioctls, and the seccomp filter descriptor. `open_pinned` in `linux_mount_view` has a function-level `#[allow(unsafe_code)]`. `seatbelt` has its own function-level allow. Linux modules are `cfg(target_os = "linux")`.

Parent resolution is `sandbox_helper_path` wrapping `crate::install::trusted_sandbox_helper_path`. The stem is `SANDBOX_HELPER_STEM`. Inherited environment, `PATH`, and cwd are not part of the rule. `scripts/check_sandbox_contract.py` rejects a `CODEGG_SANDBOX_HELPER` identity, stderr marker parsing, and a PATH lookup inside `sandbox_helper_path`. `scripts/check_sandbox_policy_wiring.py` requires the helper to call `apply_backend(&spec)` and rejects a direct `apply_landlock(&spec)`. Both guards hardcode `src/bin/codegg-sandbox-helper.rs`.

Status encoding is a 4-byte big-endian length prefix plus a JSON `SandboxStatusFrame { version, outcome }`, capped at 16 KiB. `encode_process_scope_abi_record` writes a one-line JSON object whose key is `codegg-landlock-process-scope-abi`. Existing tests round-trip produced frames and reject truncation, concatenation, and oversize. They do not freeze the bytes against a fixture independent of the encoder under test. This milestone adds that fixture, captured before the move.

Production root imports inside the sandbox module are `ToolError`, the install path wrappers, and `managed_process::protected_mount_view_limits(&spec)` from `backend.rs`. A `crate::tool::bash::process::shell_argv` use in `policy.rs` is inside a `#[test]`. `codegg_core::approval::SandboxProfile` is already the profile type. The sandbox library may depend on `codegg-core`. It must not depend on the root package.

The hosted helper step is `cargo build --locked --bin codegg-sandbox-helper` followed by a copy to `target/debug/deps/codegg-sandbox-helper`. Product builds still resolve only the installation-owned sibling. `scripts/check_cargo_profile_alignment.py` names that invocation. `scripts/detect-live-eggwork-changes.sh` names the helper path. Release scripts key off the stem, not the source path.

Prior hosted observation, not re-measured here: warm run `44837892044` spent 3m 11s of a 4m 10s helper step in root codegen. This plan does not promise that saving. Closure measures the step again and reports whether package `codegg` was compiled.

## 4. Invariants that must not regress

- The helper package does not depend on package `codegg`.
- The root package does not own a bin named `codegg-sandbox-helper` and does not keep `src/bin/codegg-sandbox-helper.rs`, including as a shim.
- `SANDBOX_STATUS_VERSION` is unchanged. Baseline status-frame bytes, launch-spec JSON, and the process-scope ABI record line match the moved encoder.
- Helper identity remains the installation-owned sibling. No `PATH`, cwd, or `CODEGG_SANDBOX_HELPER` fallback is added.
- `apply_backend` remains the helper's dispatch. A direct single-backend apply stays forbidden.
- Missing helper, empty status, truncated status, duplicate terminal status, and sandbox-unavailable constrained execution stay fail-closed. No uncontained spawn is added.
- Reviewed `unsafe` does not move into `codegg-core` or into the root library. The sandbox library denies `unsafe_code`. Function-level allows stay on the functions that already have them.
- Linux-only modules stay `cfg(target_os = "linux")`.
- Writer-fence, hostile-Git, and queue-transaction tests are not edited to go green.
- The hosted nextest command is not reduced. The helper step still builds this bin and still copies it where the integration tests load it.

## 5. Scope

### In scope

- A workspace library for the helper-facing contract, and a bin package that owns `codegg-sandbox-helper`.
- Moving the helper source and the contract it shares with the parent. Root re-export of the library types so existing `crate::security::sandbox` callers keep compiling.
- Frozen-byte fixtures captured from the baseline encoder before the move.
- A dependency-direction guard with `--self-test`, wired into `quick` and CI.
- Path updates in existing sandbox guards and in the live-eggwork detector, without weakening their checks.
- The hosted helper step's package selector, and only that change in the workflow, taken from the baseline file.
- `docs/test-ownership.toml` rows for the moved paths.

### Explicitly out of scope

- Any change to launch authority, obtained-enforcement meaning, `SandboxProfile`, `ApprovalMode`, or the default Linux policy.
- Folding this code into `codegg-core`.
- A crate-wide `unsafe` allow. A version bump of the status frame. A new status field that changes existing bytes.
- PATH lookup, a `CODEGG_SANDBOX_HELPER` override, or resolving the helper from cwd.
- Extracting `server`, `agent`, `tool`, `core`, or `scheduler`.
- The TUI cut (milestone 002), except that this milestone must not add a root dependency on a TUI crate.
- Dropping a test family, un-exclusiving a nextest target, or editing the unrelated dirty CI drafts.
- Quoting a minute saving as acceptance.

## 6. Required production changes

### Core/domain

Do not add sandbox launch code to `codegg-core`. The new library may depend on `codegg-core` for `SandboxProfile` and `NetworkIntent`. Core must not depend on the new library.

### Storage and migrations

None.

### Protocol and DTOs

No public protocol change. `SandboxLaunchSpec`, `SandboxStatusFrame`, `encode_sandbox_status`, `decode_sandbox_status`, and `encode_process_scope_abi_record` move without a schema change. Do not bump `SANDBOX_STATUS_VERSION`. Do not add a serde field that changes the bytes of a baseline value. `#[serde(default)]` on existing fields stays.

### Runtime and concurrency

The helper stays a one-shot child. Parent spawn, status-pipe ownership, and the `target/debug/deps` copy stay. `protected_mount_view_limits` is a pure derivation from `SandboxLaunchSpec`. Move that derivation with the spec, or call it from the root wrapper without making the helper package depend on `src/managed_process.rs`. Do not pull `managed_process` into the helper's crate.

### Frontend or operator surface

None. The installed stem stays `codegg-sandbox-helper`. The unavailable-platform message stays.

### Security and authorization

Fail-closed paths stay. The bin package holds the reviewed `unsafe`. The library crate has `#![deny(unsafe_code)]` and inherits workspace lints. If a single package cannot express "lib denies unsafe, bin does not", split the lib and the bin into two packages. Do not drop the deny to keep them together.

Parent-only functions stay in the root: `sandbox_helper_path`, `trusted_sandbox_helper_path` wrappers, and `ToolError` mapping for `validate_path_safety`. The library returns its own error if a moved function cannot name `ToolError`. The root maps it. Do not depend on the root error module from the library.

### Documentation and static guards

Update path lists. Add the transition guard. Do not delete a check to make a moved path pass.

## 7. Ordered work packages

### Work package A — Freeze baseline bytes

Intent:

Capture the contract from the encoder that exists at the baseline, before any type moves.

Required changes:

Add fixture files, generated by a small test or a one-shot run against the baseline encoder, covering:

- `encode_sandbox_status` for each `SandboxLaunchOutcome` variant the existing tests already construct, including enforced and setup-error;
- one `SandboxLaunchSpec` JSON value built by the existing spec constructor, not a hand-written subset;
- `encode_process_scope_abi_record` for a fixed ABI value the existing tests already use;
- the version byte and the 16 KiB cap constant's numeric value.

The fixture commit is the input to the later equality test. Regenerating it from the moved encoder is a failed test, not a refresh.

Acceptance evidence:

The fixture file is in the tree before the move commit, and a test at that commit round-trips it through the still-unmoved encoder. The closure names both commits.

### Work package B — Cut the package

Intent:

Give the helper a package whose dependency closure does not include `codegg`.

Required changes:

Create the workspace member or members ADR-0064 requires. Move the helper bin. Move the helper-facing contract. Leave parent-only install resolution and `ToolError` mapping in the root, calling the new library. Re-export library types from `crate::security::sandbox` so callers do not all change in this milestone. Do not re-export the bin, and do not leave `src/bin/codegg-sandbox-helper.rs` in place.

The test in `policy.rs` that calls `crate::tool` moves to the root package if the sandbox library cannot see that module. It is not deleted.

`open_pinned` moves with the Linux mount-view code, still function-level allowed, still `cfg`-gated. The helper bin calls it through the library.

Acceptance evidence:

`cargo metadata` shows the helper bin's package does not depend on `codegg`, and the root package has no bin target named `codegg-sandbox-helper`. `cargo build --locked -p <helper-package> --bin codegg-sandbox-helper` succeeds without compiling package `codegg`. The closure pastes the package list from that build, not a paraphrase.

### Work package C — Keep the parent contract

Intent:

The parent still finds an installation-owned sibling and still fail-closes.

Required changes:

`SANDBOX_HELPER_STEM` stays. `sandbox_helper_path` stays a wrapper over the install rule, with the same error-wording substitution. The CI step builds the new package's bin and still copies it to `target/debug/deps/codegg-sandbox-helper`. Do not point product resolution at `target/debug/deps`.

Update `scripts/check_cargo_profile_alignment.py` so it recognizes the package-qualified invocation and still rejects a profile drift. Do not delete the check.

Acceptance evidence:

Existing helper-path tests pass. The contract guard still rejects a PATH lookup and a `CODEGG_SANDBOX_HELPER` read. A missing sibling still fails closed in the existing test.

### Work package D — Transition guard

Intent:

Make the cancelled-cut failure mode a guard failure.

Required changes:

Add a guard, with `--self-test`, that fails when:

- the package owning bin `codegg-sandbox-helper` depends on package `codegg`;
- the root package has that bin, or the file `src/bin/codegg-sandbox-helper.rs` exists;
- the helper package is `codegg-core`;
- the sandbox library lacks `#![deny(unsafe_code)]`, or a crate-wide `allow(unsafe_code)` covers it.

`--self-test` injects each of those defects into a fixture and proves the guard rejects them. Wire the guard into `run_guards()` in `scripts/verify.sh` and into `.github/workflows/ci.yml`. A guard that runs in only one of those is not done.

Update `scripts/check_sandbox_contract.py` and `scripts/check_sandbox_policy_wiring.py` to the new helper path. Their forbidden-pattern checks stay. Update `scripts/detect-live-eggwork-changes.sh` the same way. Grep the tree for `src/bin/codegg-sandbox-helper.rs` and retarget every guard. A doc that only mentions the stem need not change.

Acceptance evidence:

`--self-test` fails on each injected defect and the uninjected guard passes. Both `quick` and the CI workflow list the new guard. The old path is absent from guard file lists.

## 8. Failure, cancellation, restart, and contention semantics

A missing helper remains an error, not an uncontained launch. A status stream that fails `decode_sandbox_status` remains an error. The helper still treats a missing status frame as the parent already does.

This milestone does not change cancellation or restart. A half-moved tree, where the parent looks up the helper by `PATH` because the sibling copy was forgotten, is a failed implementation. Do not add that lookup to get a local test running.

Two concurrent helper builds are ordinary Cargo contention. No new lock is introduced.

## 9. Compatibility and migration

No data migration. Installed stems stay `codegg`, `codegg-sandbox-helper`, and `codegg-eggsearch`. Upgrade membership checks that list those stems stay.

Old status bytes must decode. New bytes must equal the fixture from work package A. A parent built at the baseline and a helper built after the move, and the reverse, are the compatibility pair the fixture stands in for. Do not require a live cross-binary run if the fixture is the exact encoder output and the decoder tests still reject truncation and concatenation. Do require the fixture.

`cargo build --bin codegg-sandbox-helper` from the workspace must still select the bin by name. Package-qualified invocations in CI and guards are updated in the same change. `cargo build -p codegg --bin codegg-sandbox-helper` must fail after the cut, because the root no longer owns it. That failure is intended. Update callers; do not add a root bin to keep the old invocation.

## 10. Required tests

### Focused unit tests

- The frozen-byte equality test from work package A, running against the moved encoder.
- Existing encode/decode tests, moved with the type: truncation, concatenation, oversize, and round-trip of a produced frame.
- `open_pinned` refusal test that already exists (`open_pinned_refuses_a_grant_path_that_vanished`), still compiled only on Linux.
- The moved `policy.rs` test that needed `crate::tool`, still asserting the same workspace-write root distinction.

### Integration tests

The constrained integration tests that load `target/debug/deps/codegg-sandbox-helper` stay in the workspace nextest command. Identify them from `docs/test-ownership.toml` rows for `src/security/sandbox.rs`, `src/security/sandbox/**`, and the helper path, and run those targets after the move. Update the ownership rows in the same change. Do not delete a row.

### Restart and recovery tests

None new. Existing helper-missing and empty-status tests cover the fail-closed restart of a launch. Do not add a daemon-restart scenario; the helper is not a daemon.

### Contention and cancellation tests

None new. Do not weaken existing scheduler cancellation coverage if a sandbox test lives in that family. Run the family's existing tests if ownership says the moved files select it.

### Security and negative tests

- Contract guard and policy-wiring guard, including their existing `--self-test` if they have one, plus the new transition guard's `--self-test`.
- Existing tests that a constrained sandbox-unavailable request does not spawn uncontained, and that Bash rejects `DegradedUncontained` before a managed spawn.
- A negative test that production helper resolution does not read `CODEGG_SANDBOX_HELPER`. The contract guard already scans for this; keep that scan on the new files.

### Migration and compatibility tests

The frozen fixture is the migration test. It must fail if the moved encoder emits different bytes. Do not mark it `#[ignore]`.

## 11. Required verification commands

Run the narrow tests first, then guards, then `quick`. Do not use `scripts/verify.sh scoped` as the iteration loop. Do not use `iter` as a gate. Do not pass `--all-features`.

```bash
# after the move; package name is the one this milestone creates
cargo test -p codegg-sandbox --lib
cargo test -p codegg --lib sandbox::

python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_sandbox_helper_package_cut.py --self-test

scripts/verify.sh quick
```

The guard filename may differ. The `--self-test` and the `quick` plus CI wiring may not.

Hosted closure, not a local substitute:

- one warm run of the helper step after the cut, with the log showing package `codegg` was not compiled by that step;
- the run id recorded in the closure;
- no claim that one run's wall clock is a precise effect size.

If the helper step still compiles package `codegg`, the milestone is not done. Do not "fix" that by dropping the step.

## 12. Documentation updates

- `architecture/tool.md` or the sandbox architecture note, if one states the helper is a root-package bin. State the package direction and the unchanged stem. Do not restate ADR-0064.
- `docs/test-ownership.toml` for moved paths.
- Guard comments that name `src/bin/codegg-sandbox-helper.rs`.
- Do not edit `AGENTS.md` except to correct a sentence this move makes false. The planning worktree's `AGENTS.md` is dirty with unrelated drafts. Edit from the baseline version of that sentence only, or stop.
- Do not add a skill.

## 13. Acceptance criteria

- `cargo build --locked -p <helper-package> --bin codegg-sandbox-helper` does not compile package `codegg`.
- `cargo build --bin codegg-sandbox-helper` from the workspace still produces `target/debug/codegg-sandbox-helper`.
- The installed stem, the sibling resolution rule, and the fail-closed missing-helper behavior are unchanged.
- Baseline status-frame, launch-spec, and process-scope ABI fixtures match.
- The transition guard is in `quick` and CI, and its `--self-test` rejects an injected root dependency and an injected root bin shim.
- No sandbox, containment, writer-fence, hostile-Git, or queue-transaction test was deleted or had an assertion removed.
- The hosted nextest command is unchanged.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- ADR-0064 is still proposed.
- The helper-facing surface cannot be separated from a root-package dependency without moving `install`, `tool`, or `managed_process` behavior into the helper package. Inverting `protected_mount_view_limits` is in scope. Depending on the root is not.
- Byte equality with the baseline fixture cannot be kept without a version bump or a new field. That is a protocol change. Stop.
- A guard cannot follow the new path without losing a check. Stop. Do not delete the check, and do not put a shim file back.
- The worktree's unrelated CI or `verify.sh` drafts cannot be kept out of the commit. Stop.
- The only way to go green is to drop a test, a family, or the hosted helper step. Stop.
- The cut seems to require folding `unsafe` into `codegg-core`, or a crate-wide unsafe allow. Stop.

## 15. Closure evidence required

- The fixture commit and the move commit.
- `cargo metadata` or the build log proving the helper package's closure excludes `codegg`.
- Guard `--self-test` output, and the `quick` and CI wiring diff.
- Commands from §11 with pass/fail, labeled local. Hosted helper-step run id, labeled hosted, including whether package `codegg` was compiled.
- A statement that no test assertion was removed, with the ownership-toml diff as the path evidence.
- An explicit note that this milestone was not expected to move the 7-minute `cfg(test)` compile, and that a null effect on that compile is not a failure of this milestone.

## 16. Handoff notes

Start from a clean tree at the baseline, or isolate edits from the unrelated drafts named in the header. Register no status change to `active` until a maintainer accepts ADR-0064 and implementation actually starts. The registry row stays `proposed` until then.

The TUI milestone is independent. Do not start it from this plan. Do not expand into agent, tool, core, or scheduler extraction if the helper step's saving is smaller than the prior observation.
