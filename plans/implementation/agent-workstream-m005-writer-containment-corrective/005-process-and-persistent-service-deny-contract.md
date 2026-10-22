# M005 Writer-Containment Corrective C005 — Process-Lifetime Denials and Platform Gate (Q3–Q4)

Status: closed — C005 closed (`plans/closure/agent-workstream-m005-writer-containment-corrective/005-status.md`); C004 closed (`plans/closure/agent-workstream-m005-writer-containment-corrective/004-status.md`); ADR-0047 acceptance inherited
Primary class: invariant / capability
Planning baseline: plan/agent-workstream-m005-writer-audit audit 2026-10-19
Source: plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md C005
Original: plans/implementation/agent-workstream-blackboard/005-group-worktree-placement.md persistent-process/native lifecycle record and Q3–Q4.
ADRs: accepted ADR-0002/0032 and **accepted ADR-0047 (option B, 2026-10-19)**; upstream predecessor closures are still required.

## Objective

Make the *restricted* shared checkout profile genuinely closed to uncontrolled processes. Deny any command, shell/Python/formatter/test/build, PTY, Tool Program process delegation, plugin/hook, LSP/MCP persistent process or other executable surface that cannot prove full checkout-scoped writer lifetime. Initial v1 is intentionally native file/Git-only. Future isolated-worktree agents still keep their established process tools and sandbox ceilings. Native platform evidence must be accurate rather than equating child exit with process-tree quiescence.

## Source evidence

The audit lists src/managed_process.rs join_output warning that setsid descendants escape Unix process groups; src/interactive_process.rs holds long-lived PTYs and only process-slot admission; src/mcp/local.rs kills a direct persistent child; egglsp launch/restart retains server lifetime; process-backed plugin/hook adapters have separate owners; src/tool/task.rs and Tool Programs may delegate scheduler jobs. macOS focused managed-process 20/20, PTY 22/22 and MCP 5/5 plus hosted Windows process-launcher and Linux full-suite tests prove limited existing lifecycle behavior, not arbitrary writer isolation.

## Work packages

A. Build a capability matrix by *effective runtime effect*, not advertised tool name or model role. At child/admission/tool factory creation preflight, strip/deny all process-capable tools and long-lived capabilities for the restricted shared group. Recheck at ToolBroker final dispatch and at JobScheduler/final executor, including compatibility/direct job clients and routes outside ToolBroker. Reject unsupported process launch even when a tool is labeled ReadOnly or plugin metadata lies; do not rely on command-syntax regex, arbitrary path globs or prompt instructions.

B. Resolve checkout scope, group participation and sandbox policy from daemon trusted records. No model argument can opt a process back in. Nested subagents and Tool Programs inherit the narrow profile; switching an execution target, background mode, human shell or tool version cannot silently widen it. Fail the spawn/transaction before it creates a process or writer side effect. Distinguish a CodeGG human originating a process from an unrelated external OS process.

C. For legitimate readonly functions that require a persistent local LSP/MCP process, require either execution in isolated mode or a separately qualified read-only filesystem backend and lifetime supervision. Do not pretend the host applyEdit refusal makes the LSP child read-only. No PTY lifetime restriction by simply releasing the permit on the first tool return; refuse in v1.

D. Separate native future feasibility from current activation: add adversarial isolated-host fixtures proving detached setsid descendant ability to outlive parent cleanup, for Unix where supported. Record Windows job-object child/process-tree observations distinctly from Linux/macOS; a 'Windows Job Object' or Unix PGID does not itself enforce filesystem access restrictions. Experimental process-tree containment improvements may be documented but must not become capability-positive without positive quiescence tests. Keep no additional OS isolation backend in this bounded corrective.

E. Guard against existing persistent process started *before* the shared reservation: preflight must detect or conservatively reject group reservation on an active affected checkout, rather than leaving a hidden LSP/MCP/PTY/process writer running with old unrestricted authority. Check async shutdown/restart/cancellation and inflight dispatched requests for races.

## Verification and acceptance

- Denied pre-effect: bash/terminal/Python/test/build/format, job submissions, Tool Program delegation and nested jobs, PTY start/attach, LSP/MCP local server launch/use, hooks, process plugins, human shell on the same reserved checkout. Test both direct broker and scheduler path.
- Preexisting persistent process or pending process attempt blocks reservation unless demonstrably drained before admission.
- Two separate worktrees still run ordinary shells concurrently under the previously permitted profile.
- No broad-blocker regression to ordinary unrestricted/isolated coding agent tool use; missing typed scope for claimed shared mode refuses.
- Linux/macOS/Windows capability/denial matrix based on actual native tests. Never state descendant quiescence for failed or unrun adversarial cases.
- Existing cancellation and quota regression suites stay green. Run focused managed_process/interactive_process/mcp, scheduler/executor/tool factory tests, source execution ownership guard, fmt, quick, Clippy and existing hosted native jobs where relevant.

## Stop and closure

Stop C005 and keep shared mode unavailable if ANY process or persistent service can escape a denial, if active preexisting writer can survive reservation, or if baseline isolated process execution regresses. Full process-tree containment is NOT required to pass restricted v1 if every unqualified process surface is correctly denied before use. Write plans/closure/agent-workstream-m005-writer-containment-corrective/005-status.md with permitted/denied matrix, exact native host evidence, escaped-descendant findings, unchanged default behavior and C006 unblocking audit.
