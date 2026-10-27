# Harness Lab Mainline Qualification H003 — Coherent Long-Horizon Rust and Job Fixtures

Status: **closed for offline native qualification** — see `plans/closure/harness-lab-mainline-qualification/003-status.md`.
Source roadmap: `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md`.
Reviewed baseline: `plans/harness-lab-continuation-20261026@cd9fde42491c85941c31efbc2d5d200e8db4d638`.
Existing owners: `plans/subsystems/long-horizon-work-execution-roadmap.md` (M001–M005 closed), `plans/subsystems/agent-loop-operational-reliability-roadmap.md` (closed), CodeGG job scheduler/tool-programs and worktree/agent workstream plan lines.
Long-term: `plans/000-long-term-specification.md` §§16,24,26,28–29; `plans/003-planning-process.md`; accepted ADR-0003, ADR-0040. Primary class: capability (benchmark fixture and verifiers), not production scheduling.

## 1. Objective

Provide a **genuinely difficult, coherent multi-step agent workload** that exposes CodeGG runtime weaknesses a 63-trial `worklog/ranges` sample cannot: reading across a sizable project, multiple source edits, compiling, interpreting failure, recovering after tool errors, waiting on a long asynchronous job, context/goal continuation, and verifying the result. Keep it reproducible, isolated, independent of model judgment and inexpensive in credential-free CI.

Existing `read-search-patch-rust`, `compile-debug-rust` and `background-job-wait` v1 cases are already supported, verified fixtures. **Do not recreate those as “new coverage.”** This is a deeper *single scenario* that crosses those boundaries and tests transitions over time. The two deferred scenarios (`delegated-nonconflicting-workstreams`, `mcp-lsp-integration-isolation`) remain deferred until their actual products/interfaces and independent verifiers are qualified; they are not acceptance requirements here.

## 2. Provider constraint

Only **CodeGG `opencode_go`** may be used in an optional later H004 live run. Its upstream serving model(s) are a separate routing dimension, **not separate provider integrations**. H003 itself is offline: deterministic fixture/fake provider/recorded ACP paths; no live calls, no OpenCode CLI proposer dependency and no subscription-based assumptions.

## 3. Invariants and fixture boundaries

- Fresh lane-private daemon, session, workspace, sandbox, scratch and test target/output; artifact and test caches do not contaminate separate candidate/baseline lanes or worktrees. Use a common **immutable input fixture snapshot**, not an accidental shared mutable Cargo target.
- One scenario should require a chain of actions: inspection -> hypothesis -> edits -> compile -> failure diagnosis -> second edit -> test -> background task status/wait -> final hidden verifier. A turn count is not a substitute for correct work. Any task that can be solved by writing a known final file in one step is too shallow.
- Hidden evaluator and fixture digest live outside writable workspace and cannot be changed by agent or candidate; detect disabled/rewritten tests, deletion, use of bypass tools and inadvertent network dependency. Use dependency-free Rust crates or offline vendor/cache carefully; no user-machine or SBC compile requirement.
- Require a **real CodeGG-owned** background job/correlated task and official status/completion notification, not a fake script/polling loop dressed as a job. Test that other permitted work proceeds while compilation is active and that completion is attributed to the original job ID.
- The run must preserve terminal truth under failed tool calls, event gaps, typed policy Deny, cancellation, and job/restart ambiguity. Missing observation is unmeasured. Never replay an acknowledged mutating operation on restart.
- Per-workspace build fairness: concurrent agent/job compiles do not repeatedly compete for a shared target lock; isolate target dirs or explicitly admit serialization; capture `cargo`/rustc timings and contention outcome. Do not install Rust toolchain on SBCs to satisfy CI.
- Pure scenario correctness separate from telemetry availability: compare actual code/test outputs and hidden verifier first, then diagnostics, wallclock, tokens and job lifecycle; unavailable CPU/RSS/context stays null, not zero.

## 4. Dependencies / scope

Ready on closed Long-Horizon M001–M005, existing CodeGG tools and D001–D003. H002 is a **soft interface** for future score schema; fixture coding, fail-first verifier and CI offline work may proceed independently. H001 is an **operational dependency** only for later real-main qualification. No need to enable experimental delegated worktrees, multiple models, multiple adapters, MCP, LSP or remote Eggwork node.

## 5. Ordered implementation packages

A. **Capability inventory:** inspect current `dev/harness_lab/fixtures/agent_autonomy/cases.jsonl`, `autonomy_run.py`, `sustained.py`, current CodeGG native background job and Tool Programs capability, context-epoch/WorkPlan operations, and `tests/tool_programs`. Determine exact supported tool route for asynchronous compilation, wait, cancel and restart; do not invent job API names. Pin actual session/turn/job identity and any needed effective sandbox.

B. **Construct coherent Rust repair task:** use a small **multi-module but substantial** Rust workspace with a real compile error and multi-stage logic failures in separate modules. Freeze baseline source, expected type/behavior contract and hidden independent verifier. A partial fix may pass public tests but fail hidden tests. Avoid tests whose solution is embedded in the prompt or whose final output is only prose. Include one long compile or deliberately slow *valid* job and independent other work to do while it runs.

C. **Offline scripted native ACP fixtures:** implement deterministic provider scripts for two opposite trajectories: (i) correct workflow with provider/tool/job interactions, (ii) wrong shortcuts such as synchronous shell waiting/polling, duplicate job launch, test deletion, incorrect final code, or skipping recovery. Verifier must fail the wrong path with a **named reason**, not just absence of a summary. Ensure the native broker executes the actions (not Python writing directly to pretend the model used a tool).

D. **Long-running transitions:** verify a continuation across context epoch/compaction or a documented turn boundary; a background job completing while the agent continues; one correct tool failure and recovery without false stall; a cancellation/restart branch that produces terminal/budget/unknown truth and no orphan process. Keep these transitions within product-supported contracts and mark deferred ones honestly.

E. **Measurement/report schema:** frozen case ID/digest and implementation SHA; single coherent task identity, per-step evidence and attempt ID, exact artifact hash, Cargo target placement, tool/job count, provider request count, test verifier result, observer gap health, context transitions, runtime wallclock, provider usage with provenance, and cleanup. Include no raw secrets or unrestricted transcript dumps. If a feature doesn't expose a metric, record `unavailable` and its evidence source.

F. **Cross-lane and negative controls:** verify two independent CodeGG lanes can run offline simultaneously without shared target/write corruption; a failed/cancelled lane doesn't block the other; protected inputs/hidden verifiers remain immutable; a stale job completion cannot be attached to another session; effort remains stable under cold/warm cache variants (label rather than pool them).

G. **CI / docs:** run focused tests and new guard proving case verifier stays outside candidate/workspace writable set; ensure test uses fast deterministic or simulated delay under CI and place actual multi-minute compile only in optional manual smoke. Do not add routine paid model calls, an extra permanent slow CI stage or a compiled fixture that requires external network.

## 6. Verification

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
cargo test -p codegg --test acp_stdio
cargo test -p codegg --test tool_programs
cargo test -p codegg --lib agent
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

Focused exact path filters may differ; show discovered/executed count. Verify before/after hidden case: unsolved fails, correct fix passes, test-tampered case fails, fake completed job without matching job ID fails, forked detached compile fails cleanup. If product lacks live job wait in relevant mode, this is a **feature gap** with a separate production corrective owner: keep fixture blocked, don't fake job execution.

## 7. Failure/cancellation/concurrency

The controller must enforce request, tool-call, wallclock and owned-daemon process ceilings. A timed-out compile is not evidence of stall if still valid; a failed compilation is a tool failure the agent can correct. Recover a daemon crash only within actual product replay contract; do not blindly replay writes/build jobs because the ACP notification was lost. Clean up process group, socket, cargo lock/target and scratch for each lane, verifying no surviving child. Refuse any workspace root crossing tool/permission boundary.

## 8. Acceptance / stop / closure

H003 closes when the frozen coherent Rust/job case and its hidden verifier are implemented and tested on **real native CodeGG ACP/tool/job paths** in both passing and negative trajectories; it exercises at least one asynchronous job completion and subsequent code/test repair, and tests two isolated subjects. A deterministic fake provider may drive the entire fixture; H004 later supplies real-model evidence.

Stop if native public job API is unavailable or production changes require opening scheduler/Tool Programs design; hand a failing test to the owning subsystem. Do not promote the deferred MCP/LSP/delegation cases or claim long-horizon model reliability from offline scripted success. Write `plans/closure/harness-lab-mainline-qualification/003-status.md` with fixture digests, negative controls, actual tools/job IDs, tests/CI, resource/cleanup and deferrals; update registry and roadmap only after verification.
