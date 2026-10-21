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
`codegg exec` (the headless CI entry point) and `codegg run` (the one-shot
path) build the agent loop with `AgentLoop::new(...)` and never install a
workspace path policy. The tool broker therefore refuses EVERY workspace-scoped
tool call with:

    permission denied: verified tool authority is missing workspace path policy

The daemon and ACP paths are unaffected: `src/agent/agent_loop_factory.rs`
installs `codegg_core::workspace_services::WorkspacePathPolicy::for_root(root)`
for a runtime that has no workspace services bundle. The headless paths never
do. A live headless run made five different tool attempts and all five were
refused the same way, so the agent could not read or write anything.

FAILING TEST (already written, currently red)
    cargo test --test exec_workspace_path_policy
It spawns the real `codegg exec` binary against a local fake provider, asks
the scripted model to `read` a relative marker file inside the workspace, and
asserts the tool result contains the marker body. It fails today because the
tool call is refused before any result is produced.

ALLOWED PATHS (touch nothing else)
    src/exec.rs
    src/main.rs
    src/agent/agent_loop_factory.rs

REQUIRED
1. Make that test pass. Install the same single-root workspace path policy the
   daemon-side factory installs. Do not invent a second policy authority, do
   not relax the broker's check, and do not weaken any other tool's authority.
2. Do not modify `tests/exec_workspace_path_policy.rs`.
3. Do not touch `dev/harness_lab/`, `plans/`, `crates/codegg-core/`, or any
   provider crate.
4. Keep the daemon and ACP paths' behaviour unchanged.

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
