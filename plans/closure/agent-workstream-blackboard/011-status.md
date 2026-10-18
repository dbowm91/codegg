# Agent Workstream Blackboard M011 — Root Turn Repository Identity Corrective

Status: closed
Source implementation plan: `plans/implementation/agent-workstream-blackboard/011-root-turn-repository-identity-corrective.md`
Source subsystem roadmap: `plans/subsystems/agent-workstream-blackboard-roadmap.md`
Implementation commit: `3fbb3dab1121940ea4f5f19834e3bba9f9b06cdb`

## Finding and correction

During M008 S1 preflight, a repository-backed managed session produced delegated tasks with no repository identity. `CoreDaemon` had hardcoded `TurnRunInput.repository_id` to `None`; the executor only provisions an isolated child worktree when the durable task carries a repository ID. This meant the M006 isolated session launch guarantee did not propagate to delegated mutation.

Root turn construction now reads the authoritative binding for the session runtime's `WorkspaceId` and passes the repository ID only when the binding is resolved. Repository-less or unresolved bindings preserve the existing no-repository behavior. The lookup is read-only and creates no checkout. M005 shared-checkout execution remains unavailable.

## Evidence

| Requirement | Evidence | Result |
|---|---|---|
| Resolved session workspace repository identity reaches the runtime | `core::daemon_turns::tests::repository_binding_reaches_turn_runtime_for_isolated_delegation` | pass, 1/1 |
| The propagated field controls managed child checkout creation | `src/scheduler/executors.rs`: repository-less tasks retain the in-place fallback; a task with `repository_id` enters `WorktreeService` managed checkout allocation | source reviewed |
| No caller-provided path or prompt controls repository identity | `ProjectStorage::workspace_binding(&runtime.workspace_id)` is the sole lookup source | pass by source review |
| Shared checkout stays gated | M005 remains active/blocked for shared-write capability under ADR-0035 | unchanged |

Focused test command:

```text
RUSTC=/Users/davidbowman/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/rustc RUSTDOC=/Users/davidbowman/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/rustdoc CARGO_TARGET_DIR=/tmp/codegg-arm-target CARGO_INCREMENTAL=0 /usr/local/bin/cargo test --locked --target aarch64-apple-darwin --lib core::daemon_turns::tests::repository_binding_reaches_turn_runtime_for_isolated_delegation -- --exact --nocapture
```

Result: 1 passed. The test creates a local repository with a remote, registers its workspace/project through daemon APIs, creates a session, reads the durable resolved binding, submits a turn through the daemon, and asserts the recording runtime receives exactly that repository ID.

`cargo fmt --check`, `git diff --check`, `python3 scripts/check_execution_ownership.py`, `python3 scripts/check_scheduler_bypass.py`, and `bash scripts/check-core-boundary.sh` all passed. The Rust build emitted existing toolchain warnings because `rust-objcopy` cannot load `libLLVM.dylib`; linking also warned that `__eh_frame` exceeded the compact-unwind table limit. The test binary linked and the focused test passed.

## Limits and disposition

This closes the discovered propagation defect. It does not establish that a provider-backed child completes or improves agent effectiveness. M008's S1 attempts using `spawn_many` created distinct child task/worktree records after this correction, but the child journals ended at `safe_boundary: before_provider_turn` without provider usage or completion evidence; the records do not establish whether a request was submitted. They were cancelled through normal scheduler `JobCancel`; no child changes were integrated. Those trials are excluded from success and performance denominators.

## Downstream audit

M008 remains active and is not unblocked further: its S0 shared-interface baseline now has one completed provider-backed run, but a completed multi-agent isolated comparison and remaining correctness strata are still required. M005 remains active feasibility work; M011 supplies no writer-containment evidence and does not unblock S4. No other registered plan depends on M011. M006 remains historically closed; this sibling corrective records the newly discovered root-turn-to-child identity gap without rewriting its original closure.
