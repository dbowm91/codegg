# Managed Agent Scratch M003 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/managed-agent-scratch/003-permission-guidance-and-lifecycle-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`

Repository baseline reviewed: `d83dfda1a6cf3676e6d221b10dd1fe080eb84794`

Implementation commit:

- `d83dfda1a6cf3676e6d221b10dd1fe080eb84794` — advisory temp-write recovery, race-aware Unix shared-scratch reclamation and retry, platform-accurate guidance, and tests.

## 1. Executive finding

M003 is conditionally closed. Structured writes to a recognized host temp root can suggest the current private scratch path after workspace authorization rejects the requested path. The tool does not rewrite or perform the write, does not expose shared roots, and preserves the existing permission and sandbox decisions. Workspace writes remain authorized even when the workspace itself is nested under the host temp directory.

On Unix, revoked shared scratch is atomically claimed under an opaque tombstone, checked against its typed owner record, and traversed without following symlinks. Hardlinked files, symlinks and non-regular entries quarantine the tombstone. Interrupted cleanup is retried during scheduler recovery, and in-process cleanup is serialized. On non-Unix platforms cleanup returns a typed unavailable error and retains revoked data: stable hardlink-count inspection is not available at the repository's Rust 1.89 minimum, and the Windows cross-check could not compile a C dependency without MSVC headers. Linux and Windows native runtime behavior remains unqualified.

Session and run-private roots remain disposable per-user cache data. CodeGG does not run a recursive collector or configure retention for those roots; no usage or retention measurement was taken. This keeps deletion conservative and avoids scanning on tool hot paths, with disk cleanup left to host cache maintenance.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Known temp write offers private scratch without rewriting | `tool::write::delegated_scratch_tests::unauthorized_temp_write_suggests_private_scratch_without_rewriting`; `tool::util::tests::temp_guidance_uses_path_components_and_rejects_traversal` | pass | Original file and scratch remain unchanged; traversal, root-only and prefix lookalikes are rejected. |
| Actual temp reads and workspace writes retain their semantics | Same write test; `workspace_write_under_host_temp_is_not_misclassified_as_unauthorized` | pass | Read targets the real temp file; a workspace nested under the host temp root remains writable. |
| Explicit tool/path Ask and Deny precedence remain authoritative | `permission::tests::trusted_root_write_relaxes_only_the_default_ask`; `tests/tool_execution.rs::test_bash_explicit_deny_wins_and_allow_excuses_heuristic` | pass | Explicit tool/path Ask and Deny remain in force; only the general default Ask is relaxed for trusted workspace/scratch roots. |
| FullHost shell temp access is not represented as intercepted | `src/tool/mod.rs` FullHost wiring; `architecture/permission.md`; `docs/tools.md` | pass | M003 adds no shell path scanning or rewriting. FullHost has no CodeGG filesystem containment; syntax inspection is advisory. |
| Revoked cleanup refuses links and ambiguous trees | `scratch::tests::revoked_shared_cleanup_quarantines_symlinks_and_hardlinks` | pass on Unix | External symlink target and hardlink content remain intact; the unsafe tree remains under a tombstone. |
| Daemon interruption and concurrent cleanup | `scratch::tests::revoked_cleanup_recovers_an_orphaned_tombstone_after_restart`; `scratch::tests::concurrent_revocation_cleanup_reclaims_one_resource_once` | pass on Unix | Restart is simulated by leaving an orphan tombstone; two concurrent callers reclaim one resource once. Scheduler startup recovery revisits revoked shared resources for recovered runs. |
| Attempt lease, revoke and retry ownership | `codegg-core` agent-run shared-scratch lease tests; M002 TaskTool admission and sandbox tests | pass | Revocation blocks new lease admission; an admitted attempt keeps its lease until release. Cleanup follows final lease release. |
| Native sandbox containment | `cargo test -p codegg --test sandbox_containment --locked` | pass on x86_64 macOS | 16 tests passed, including delegated shared scratch and workspace containment. No Linux or Windows native claim follows. |
| Windows compile/runtime and safe cleanup | `cargo check -p codegg --lib --target x86_64-pc-windows-msvc --locked` | not qualified | Rust target libraries were available via Rustup 1.89, but `ring` C compilation failed because the host lacks Windows MSVC headers (`assert.h`). Cleanup is disabled on non-Unix targets. |

## 3. Production implementation evidence

- `relative_to_known_temp_root` recognizes normalized components beneath the host temp root and common Unix temp roots. Windows uses `TEMP`/`TMP`. It grants no authority and performs no path rewriting.
- `WriteTool::execute_structured` returns the hint only when the requested path is outside the tool's validated workspace root and authorized scratch roots. Reads and shell commands are unchanged; ungranted shared paths are never named.
- Unix shared cleanup renames the managed directory before inspection, validates the owner record, refuses unsafe tree entries and hardlinks, and retries a claimed tombstone after interruption. A process mutex serializes overlapping cleanup calls.
- Non-Unix cleanup returns `ScratchError::CleanupUnavailable` without mutating the shared resource. The task/scheduler paths log and defer that cleanup failure.
- No storage schema, protocol, sandbox grant, permission rule, or shell parser changed in M003.

## 4. Verification executed

### Commands run

```bash
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 cargo test -p codegg-core --locked
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 cargo test -p codegg --lib scratch --locked
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 cargo test -p codegg --lib trusted_root_write_relaxes_only_the_default_ask --locked
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 cargo test -p codegg --test tool_execution --locked
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 cargo test -p codegg --test sandbox_containment --locked
rtk env RUSTC=/Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin/rustc CARGO_BUILD_JOBS=2 rustup run 1.89-aarch64-apple-darwin cargo check -p codegg --lib --target x86_64-pc-windows-msvc --locked
rtk scripts/verify.sh quick
rtk env CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
rtk python3 scripts/check_approval_reviewer.py
rtk python3 scripts/check_approval_router.py
rtk python3 scripts/check_sandbox_contract.py
rtk python3 scripts/check_sandbox_policy_wiring.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_scheduler_bypass.py
rtk python3 scripts/check_daemon_cwd_usage.py
rtk bash scripts/check-core-boundary.sh
rtk cargo fmt --all -- --check
rtk git diff --check
```

### Results

- `cargo test -p codegg-core --locked`: 861 unit tests plus core integration and documentation suites passed.
- Final-source `cargo test -p codegg --lib scratch --locked`: 18 passed.
- Trusted-root permission test: 1 passed. `tool_execution`: 61 passed.
- Final-head native `sandbox_containment`: 16 passed on x86_64 macOS.
- Final-source `scripts/verify.sh quick`: passed, including static guards and workspace all-target check.
- Final-source workspace Clippy with `-D warnings`: passed.
- Formatting, diff, permission, sandbox, execution-ownership, scheduler, cwd and core-boundary guards passed.
- The initial Windows check used Homebrew Rust without matching target libraries; retrying with Rustup Rust 1.89 reached native C compilation and stopped at missing MSVC headers. Windows is not qualified.

## 5. Invariant review

- No absolute temp path is silently rewritten and no hint creates a grant.
- Explicit tool/path Ask and Deny remain final. Workspace and private/explicit shared scratch mutations do not inherit the general default Ask.
- The original host temp path remains the read target. Shell commands retain selected sandbox behavior; FullHost is not claimed to intercept filesystem writes.
- Cleanup reconstructs the leaf from typed `ScratchId`; model-provided paths are not cleanup inputs.
- Unix cleanup claims a tombstone before validation, does not follow symlinks, rejects hardlinks, and leaves malformed or unsafe content quarantined.
- Windows/non-Unix cleanup is disabled rather than claiming unverified link safety. Required sandbox behavior remains fail-closed and independently unqualified there.

## 6. Failure and recovery review

- Store lease checks continue to gate cleanup: revoke blocks new admission and existing attempts retain their lease until release.
- Scheduler recovery revisits revoked shared resources for recovered subagent runs. If the daemon stopped after tombstoning, a later cleanup claims and validates the orphan before retrying.
- Concurrent Unix cleanup callers are serialized. Unsafe trees and filesystem errors stop deletion and preserve the tombstone for a later retry or inspection.
- Cleanup failure is logged and deferred; it does not turn into a new grant or a write.
- No automatic recursive session/run cache collector was added. Those roots remain disposable cache data for host maintenance.

## 7. Migration and compatibility review

M003 adds no database migration, DTO, configuration key or protocol field. The default subprocess temp environment and existing scratch ownership are unchanged. Non-Unix callers receive a typed deferred-cleanup error; revoked data is retained there. No retention configuration was added because no global collector was introduced.

## 8. Security review

Temp classification is advisory, component-based and limited to structured writes. Workspace authorization is checked first, including workspaces nested under a temp root. The hint includes only the current private root. Shared path disclosures, shell parsing, symlink traversal and hardlink deletion were not added. Unix tombstone traversal is followed by `remove_dir_all`, whose path removal does not follow symlink entries; an unsafe tree is rejected before removal. The same-UID FullHost limitation remains unchanged.

## 9. Documentation and operations

Updated `architecture/permission.md`, `architecture/security.md`, `docs/tools.md`, `.opencode/skills/permission/SKILL.md` and `.opencode/skills/agent/SKILL.md`. Documentation distinguishes the advisory structured-write hint, FullHost shell behavior, Unix cleanup and non-Unix retention. No operator CLI or recursive size scan was added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Medium | Non-Unix shared cleanup is disabled; Windows MSVC cross-build stopped at missing host headers and no Windows runtime was available. | Revoked shared scratch remains on disk on Windows; native path and sandbox behavior is unqualified. | Keep cleanup disabled until stable hardlink/reparse inspection is available at MSRV and Windows native qualification passes. |
| Low | Linux native containment and cleanup were unavailable on this host. | No Linux runtime claim follows from this closure. | Qualify on a Linux host before publishing a Linux-specific enforcement claim. |
| Low | Session/run-private cache roots have no CodeGG recursive GC, retention configuration or measured usage. | Cache growth depends on host cache maintenance. | Preserve disposable-cache documentation; any collector needs a separate lifecycle/retention design and race tests. |

## 11. Roadmap disposition

M003 is conditionally closed. Its structured-write guidance and Unix revoked-shared cleanup are implemented and verified. The named platform conditions above remain explicit; they do not block already-registered downstream work because no M004 or other successor plan exists in this workstream. The managed-agent-scratch roadmap has no newly eligible milestone.

## 12. Registry updates

- Mark M003 conditionally closed in its implementation plan, subsystem roadmap and `plans/registry.md`.
- Record this exact-head closure and the conditional platform/retention evidence above.
- Re-audit found no successor milestone or other registered plan in this line to unblock.

## 13. Factual correction — authenticated native mutation defaults (2026-10-18)

The original closure described the no-prompt behavior primarily for structured
`write`. The accepted product correction is that writes inside the authenticated
workspace or authorized private/shared scratch roots must not trigger the
general permission default, including `permission.default = "ask"`. The
permission boundary now applies this behavior consistently to `write`, `edit`,
`apply_patch`, and `replace`, while retaining stored decisions, explicit
tool/path Ask and Deny rules, hard policy denials, and host path validation.
Shared scratch remains limited to explicit grants. This does not grant
scratch authority to shell, Git, another session, or unlisted paths.

Added verification:

- `permission::tests::global_ask_default_does_not_prompt_for_authenticated_file_mutations`
- `permission::tests::trusted_root_path_ask_keeps_the_mutation_tool_identity`
- Final-head `permission::tests::` suite: 62 passed on ARM64 macOS.
- `cargo fmt --all -- --check` and `git diff --check`: passed after the correction.
- `scripts/verify.sh quick`: passed on ARM64 macOS after the correction.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed on ARM64 macOS.

## 14. Factual correction — Windows reclamation and M004 successor (2026-10-19)

Later changes supersede this record's statements that cleanup is disabled on
non-Unix platforms. `f97db619` added Windows shared-scratch reclamation using
stable reparse-point inspection, and `3051ffb9` added a native reparse-point
fixture. Windows scratch lifecycle tests are run by the Windows process
launcher workflow. The Windows managed-scratch lifecycle step passed in run
[39227777443](https://github.com/dbowm91/codegg/actions/runs/39227777443),
with 11/11 lifecycle tests and 8/8 process-launcher tests. The integrated-head
qualification is tracked by M004's closure record. This qualifies the cleanup implementation only; Windows
Required-sandbox containment remains unqualified, as does Linux native
containment beyond the hosted Linux unit suite.

M004 was subsequently registered as the corrective successor for session and
agent-private reclamation and is now closed at
`plans/closure/managed-agent-scratch/004-status.md`. No later managed-agent-
scratch milestone is registered. M003 remains conditionally closed for its
native sandbox qualification conditions; its former “no successor” disposition
is historical and does not block or imply another plan.

## 15. Factual correction — Linux native scratch containment qualified (2026-10-19)

This record's earlier statements that "Linux native containment" was
unqualified beyond a hosted Linux unit suite are superseded for the
**delegated scratch grant path** only. The post-closure corrective
addendum's C001 milestone closed at
`plans/closure/managed-agent-scratch-native-qualification-corrective/001-status.md`
with native evidence: implementation `2597ec9e` with the fixture correction
`ea400ff7`, repository `CI` workflow
run [39291133332](https://github.com/dbowm91/codegg/actions/runs/39291133332)
on `ubuntu-latest`, adding `tests/linux_native_scratch_containment.rs`.

On a native Linux host with Landlock ABI >= 3, an authenticated, admitted
delegated run was shown to hold exactly the write roots its frozen grant
snapshot authorizes: the run-private root and the explicitly granted shared
root are writable, and the kernel refuses an ungranted sibling, another
session's run-private root, the scratch parent, an unrelated host temporary
directory, a pre-existing file outside the granted roots, a cross-boundary
hard link or rename, and a symlink planted inside the workspace. The
obtained status (Landlock, ABI >= 3, `no_new_privs`) was asserted on the
same launch, and a defect-injection control proved the denials were not
vacuous.

What remains unqualified, and what this correction does **not** change:

- **Windows Required-sandbox containment remains unqualified.** The owning
  `plans/subsystems/windows-native-sandbox-roadmap.md` still records M004 as
  blocked and states that constrained Windows production
  `SandboxRequest::Required` stays unavailable. The corrective C002 stopped
  at its own work-package-A condition and was recorded as a blocked closure
  at `plans/closure/managed-agent-scratch-native-qualification-corrective/002-status.md`.
- The historical M003 findings, the date-specific failed assertions, the
  macOS-only evidence base, and the original conditional disposition are
  **not** rewritten. M003 remains conditionally closed; only the Linux half
  of its native-containment condition is now satisfied.
- The Linux qualification covers the **filesystem** dimension only. It makes
  no claim about process, IPC, or network isolation, and no claim about read
  confidentiality — the read policy deliberately exposes curated toolchain
  roots.
- The historical Windows cleanup evidence (workflow `39262822248`, 11/11
  lifecycle; 8/8 launcher) remains **lifecycle** evidence and is still not
  containment evidence.
