# C003 proposer instruction — the frozen, defective case

This is the **exact prompt** handed to the OpenCode proposer for the C003
candidate. It is frozen before the proposal runs, so the instruction cannot be
tuned to make a candidate look better than it is.

- Proposer route (approved by the operator for this run): recorded in the
  proposal receipt as `proposer_model`.
- Proposer timeout: bounded by the controller.
- Allowed touched paths: `src/exec.rs`, `src/main.rs`,
  `src/agent/agent_loop_factory.rs`.

## The frozen prompt

```text
You are fixing one verified defect in the CodeGG repository.

DEFECT
The headless adapters — `codegg exec` (the CI entry point) and `codegg run`
(the one-shot path) — build their agent loop in-process and never bind it to a
workspace. Two things follow from that single omission:

1. `src/exec.rs` and `src/main.rs` build the tool registry with
   `ToolRegistry::with_config_and_search_runtime(&config, search_runtime)`,
   which leaves `ToolRegistryOptions::workspace_root` as `None`. Every
   workspace-scoped tool (`read`, `write`, `edit`, `glob`, `grep`, `list`) is
   then registered as an `UnrootedTool`, and an unrooted tool is NOT advertised
   to the model at all. A headless run therefore offers the model no way to
   read or write a file.

2. The same paths never install a workspace path policy on the agent loop, so
   the tool broker refuses every tool call with:

       permission denied: verified tool authority is missing workspace path policy

The daemon and ACP paths are unaffected: `src/agent/agent_loop_factory.rs`
builds the registry with the workspace root and installs
`codegg_core::workspace_services::WorkspacePathPolicy::for_root(root)` for a
runtime that has no workspace services bundle. The headless paths do neither.

FAILING TEST (already written, currently red)
    cargo test --test exec_workspace_path_policy
It spawns the real `codegg exec` binary against a local fake provider, asks
the scripted model to `read` a relative marker file inside the workspace, and
asserts the tool result contains the marker body. It fails today.

ALLOWED PATHS (touch nothing else)
    src/exec.rs
    src/main.rs
    src/tool/mod.rs
    src/agent/agent_loop_factory.rs

REQUIRED
1. Make that test pass. The headless adapter must bind its workspace root to
   its tool registry so the workspace-scoped tools are advertised and rooted,
   and it must install the same single-root workspace path policy the
   daemon-side factory installs. Do not invent a second policy authority, do
   not relax the broker's check, and do not weaken any other tool's authority.
2. Do not modify `tests/exec_workspace_path_policy.rs`.
3. Do not touch `dev/harness_lab/`, `plans/`, `crates/codegg-core/`, or any
   provider crate.
4. Keep the daemon and ACP paths' behaviour unchanged. The registry's existing
   constructors and their defaults must keep their current meaning.

CHANGELOG (required file)
Create `CHANGELOG.harness.md` at the repository root with exactly these four
sections, filled in truthfully:

    ## Root cause
    ## Contracts
    ## Validation
    ## Accepted risks

No placeholder text: no "TODO", no "TBD", no "pending", no "<candidate". If
you did not run a test, say so in "Validation" rather than claiming a pass.
```

## Why the prompt is frozen

C003 §5-B requires the proposer instruction to point at the failing test, the
allowed touched paths, and the risk and changelog requirements. The prompt
above is that instruction, and it is recorded here before any proposal runs so
the acceptance decision cannot be attributed to a prompt that was rewritten
after the fact.
