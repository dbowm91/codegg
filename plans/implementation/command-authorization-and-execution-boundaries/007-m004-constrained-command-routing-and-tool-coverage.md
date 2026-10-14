# Command Authorization M004 — Constrained Command Routing and Tool Coverage Successor

Status: **active**; constrained specialized routes are being closed as typed-unavailable until they can preserve accepted profiles. Closure target: `plans/closure/command-authorization-and-execution-boundaries/007-status.md`.
Class: capability / invariant
Closure target: `plans/closure/command-authorization-and-execution-boundaries/007-status.md`.
Baseline: `main` @ `ec43e495c3619966ac33d0acccf7904812dd37bc` (2026-10-13). Command authorization M001–M003 already closed; do **not** reopen their evidence. Normative accepted command-authorization ADR-0032 must be canonically renumbered without content change under Linux C004. `src/tool/authorization.rs` and `ExecutionPolicySnapshot` own accepted authority; `ManagedProcessService` owns actual child launch and obtained filesystem enforcement.

## 1. Problem and non-goal

Command authorization M002 conservatively restricts *active* Test/Python/Git/native routing to an explicitly accepted `FullHost` snapshot. A constrained accepted `WorkspaceWrite`/ReadOnly agent tool invocation goes through the raw managed Bash route to preserve real Landlock enforcement; this is secure but can leave specialized/test/typed-tool UX unavailable. Never "improve" routing by silently lifting a constrained snapshot or by trusting executable names. Address route parity only where **the same or narrower obtained rights** follow the accepted receipt through each executor.

Do not change parent `ApprovalRouter`, relax existing agent/file/Git authority, enable FullHost by default, or assert network/IPC isolation from Landlock-only.

## 2. Work packages

A. Inventory each eligible path: `src/tool/bash/process.rs` active route guard, `src/command_intent/`, `src/tool/test.rs`, `src/python_script/`, `src/tool/git.rs`, `src/git_mutations.rs`, `src/managed_process.rs`, scheduler Tool Programs, hooks/plugins, LSP/MCP, human shell, and operator-invoked TUI `/test`. Separate daemon-owned trusted mutations and explicit operator requests from model-controlled executable children; list exclusions explicitly. Record who owns authorization, working directory, environment/FDs, sandbox profile, network rights, output/timeouts, audit and fallback.

B. Define **one** route-admission predicate derived from immutable accepted `ExecutionAuthorizationContext` plus actual backend capability evidence, not argv0, shell lexer, risk label or an easily forged "test tool" tag. When supported, route constrained subprocesses through **the same ManagedProcessService/legacy Landlock boundary** as raw Bash. When tool cannot preserve rights, return a typed `BackendUnavailable` and offer the already-authorized raw route only if genuinely equivalent; no silent FullHost, local Git raw fallback, credential/environment inheritance or reroute after explicit Deny.

C. Produce a deterministic behavior matrix: direct Bash versus typed Test, `python -m pytest` versus typed Python, typed/shell `git status`, filter-capable `git add` rejected until Linux C002, `rg` and native helpers, multi-stage commands, nested agents, scheduler tasks, cancellation and timeouts. A specialized surface may execute under a constrained snapshot only when it proves same-or-narrower obtained rights; otherwise it must report typed `BackendUnavailable` before job/child creation. Record requested/obtained filesystem evidence for launches and distinguish non-executable control paths.

D. Verify focused no-target failures with disabled sandbox, route-profile mismatch, stale receipt, helper/status forgery, agent child ceiling, alternate tool surfaces and attempts to access a synthetic outside-root secret. For unsupported specialized constrained routes, prove the shared admission gate rejects before scheduler submission. A special active route cannot borrow a more privileged test/MCP/LSP helper merely because it is called from an authorized Bash command.

E. Integrate with `architecture/tool.md`, `architecture/permission.md`, `architecture/process-tool-execution-ownership.md`, `docs/execution-ownership.toml`, bounded audit/status and static `scripts/check_execution_ownership.py`. No new global dispatcher or parallel process owner. Existing command-auth M001–M003 closure results remain untouched.

## 3. Order and dependencies

The inventory and production admission check use the current M002 accepted-context contract. A specialized route may be promoted only after native evidence proves it preserves current Landlock `WorkspaceWrite`/ReadOnly rights; any enhanced filesystem/proc/IPC/network promise additionally depends on Linux M003–M005 qualification. Typed Test/Python routes that do not carry the snapshot fail closed before scheduler submission. Git filter/hooks/remote commands remain typed-unavailable until their independent containment/transport authorities close. Keep route exceptions and actual reasons rather than cosmetic "all tools sandboxed" status.

## 4. Verification and closure

`cargo fmt --all -- --check`, `cargo test -p codegg --lib permission`, `cargo test -p codegg --test command_routing_adversarial`, `cargo test -p codegg --test approval_router -- --test-threads=1`, `cargo test -p codegg --test sandbox_containment -- --test-threads=1`, `python3 scripts/check_execution_ownership.py`, `python3 scripts/check_authorization_matrix.py`, `scripts/verify.sh quick` and new per-route real-child Linux negatives on an eligible host. Record retained ability to perform normal developer workflows and no syntax-only Ask regression.

Constrained Test/Python routes may close as unavailable when the typed pre-submission refusal is verified; this is a complete safety disposition, not route-parity capability. Do not claim Git or host-network isolation complete from a typed admission table. Stop immediately if a route executes under greater authority, spawns outside canonical service, loses cancellation/lease ownership, or logs secrets.
