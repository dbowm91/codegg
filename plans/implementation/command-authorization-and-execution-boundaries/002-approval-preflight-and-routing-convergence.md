# Command Authorization and Execution Boundaries M002 — Permission, Preflight and Route Convergence

Status: closed
Primary class: infrastructure
Target closure: `plans/closure/command-authorization-and-execution-boundaries/002-status.md`.
Status and acceptance are planning declarations, not execution evidence. Source roadmap: `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md`. Repository baseline: `main` 2026-10-13 and Linux branch `c7c18181862d67c9e2a649399bbda9665869edd8`. Long-term: `plans/000-long-term-specification.md` §§4,17,27–29 and `plans/001-terminology-and-domain-model.md` §8. Accepted: ADR-0001/0002/0004/0016/0024; Linux mechanism ADR-0033 proposed. Closure belongs under `plans/closure/command-authorization-and-execution-boundaries/`.

## 1. Objective

Use one stable authenticated authorization decision across command preflight, Bash/native/Python/Git routing and actual supervised execution, preventing both duplicate review prompts and privilege-laundering through a different tool or fallback. Risk signals are inputs to the reviewer and never authoritative resource capabilities.

## 2. Current evidence

`src/permission/mod.rs` resolves scoped user, path and default rules with special shell treatment; `src/permission/approval.rs` routes deterministic decisions and Automatic reviewer; `src/tool/bash.rs` performs a separate pre-spawn `check_command_security` and optional `PreflightService` before a `command_intent` plan; `src/security/policy.rs` may return Observe/Ask/Deny. Typed Git, Python and native planners have distinct entrypoints. Need inspect actual call graph and ensure no rule evaluation after routing can silently exceed the original request's authority.

## 3. Invariants and non-goals

An explicit operator Deny, job/tool disable switch, parent ceiling or real backend refusal remains terminal across every route. A syntactic risk signal never expands authority and never becomes a magic hard boundary. Exactly one logical request binds the immutable snapshot, decision receipt and selected route; a retry cannot use a newly elevated context without a new explicit approval. No command string or model-supplied option selects helper, network grant, Git credential or FullHost.

Out of scope: adding seccomp/namespace/cgroups, redesigning scheduler/agent hierarchy, default policy mode changes, treating a reviewer decision as kernel isolation, rewriting all security scanners, or adding broad session transcript logging.

## 4. Production work

A. Freeze accepted `ApprovalRouter` sources, decision receipt, parent-child scope and `SecurityService`/eggsact preflight signal semantics as test cases. Create internal `ExecutionAuthorizationContext` (reuse existing `ExecutionPolicySnapshot` and receipt where possible), carrying typed principal/workspace/worktree/run/turn, authorized tools, granted optional capabilities, required filesystem/network/process constraints and immutable generation.

B. Establish explicit precedence: verified scoped explicit Deny/ceiling/disabled tool/invalid request > approval escalation decisions (user/reviewer) within ceiling > risk classification/default auto-allow; real executor features must satisfy separately. Ensure the current safe-command auto-allow path cannot skip high-risk advice by accident. Do not let advisory patterns override an explicit narrow operator allow/deny in an undocumented way.

C. Ensure `PreflightDecision::Block` maps to terminal denial only for a provable deterministic correctness failure or contract invalidity in that tool, not a heuristic text suspicion. Typed errors must identify invalid request vs denied authority vs reviewer action vs missing sandbox. Keep eggsact runtime warnings and model-visible findings bounded/redacted.

D. Carry snapshot, authorized capabilities, selected executor and decision receipt through `src/command_intent`, `src/tool/bash/process.rs`, `src/tool/git.rs`, `src/python_script`, `src/managed_process.rs`, helper and scheduler submissions. Routing decisions are opportunistic optimization; they never alter authority; if active route is not enforceable, refuse (or select equivalent already-authorized raw route with same obtained rights, when demonstrably safe) rather than silently using FullHost.

E. Provide structured trace for request digest, classifier risk, decision source/revision, selected route, requested and obtained enforcement, and terminal outcome; avoid raw secrets, script body dumps and false assertion that child syscalls/effects were observed. Preserve existing audit APIs unless versioned.

## 5. Ordered packages and tests

1. Call-graph authority review and `bash` -> `native` / `python` / `git` fixture across all approval modes.
2. One immutable context/receipt/generation carried through routing, retry and nested child; deny route-dependent widening.
3. Normalize preflight and security outcomes, no duplicated risk scans to create contradictory denials; default "unrecognized" is not "authorized for FullHost".
4. Add typed refusal/result visibility in TUI/CLI/daemon and static guard of process owner; document exact fallback semantics.
5. Run negative/positive compatibility and approval matrix; write closure.

Tests: harmless `cargo test` invoking build script, shell `sh -c` wrapping a network or out-of-workspace write, Python `-c`, script file, typed Git operation, native binary, mixed pipelines, command-intent planner reject; explicit configured Deny, child ceiling, Automatic and YOLO cannot bypass. Test after-approval required Landlock unavailable => no marker. Test retry/timeout cancellation retains correct receipt; cannot change policy via route. Commands: `cargo fmt --all -- --check`, `cargo test -p codegg --lib permission`, `cargo test -p codegg --lib command_intent`, `cargo test -p codegg --test approval_router -- --test-threads=1`, `python3 scripts/check_approval_router.py`, `python3 scripts/check_execution_ownership.py`, `scripts/verify.sh quick`. Native host tests only where actual guarantee claimed.

## 6. Failure, compatibility, closure

No state migration by default; preserve all user/admin rule data and public approval values. If new fields cross daemon/TUI/ACP, make additive versioned changes. On unsupported execution backend, missing scope, expired generation or policy mismatch, deny before child and provide typed reason. Cancellation and reboot do not replay an approval. Document each actual route and lost capability; no silent broader retry. Closure must include tests for every protected routing transition and compare actual status to operator-facing report. Stop if any alternate executor can promote a denied capability or if the change would require model-visible secrets.
