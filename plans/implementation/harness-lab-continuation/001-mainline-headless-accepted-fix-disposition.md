# Harness Lab Continuation D001 — Mainline Headless Workspace Binding and Accepted Candidate Disposition

Status: closed
Baseline: `main@957fcbf00e17bf108429f0d75dcbf760736ae511`, 2026-10-26; refresh before implementation.
Roadmap: `plans/subsystems/harness-lab-continuation-corrective-addendum.md`.
Prior evidence: `plans/closure/harness-lab-post-pilot-corrective/003-status.md` and `003-evidence.md`, `plans/closure/harness-lab/004-status.md`. Long-term: `plans/000-long-term-specification.md` §§23,26,27,29. Primary class: invariant. No new ADR if workspace ownership unchanged.

## 1. Objective

Prove the real, accepted C003 fix for *headless workspace binding* exists on **current main**, or safely reimplement/promote it if it does not. The prior accepted OpenCode candidate was never automatically merged. Prevent a “self-improved” finding from remaining only in the private candidate worktree.

## 2. Failing evidence and scope

The accepted C003 candidate corrected `codegg exec` and `codegg run` not advertising `read/write/edit/glob/grep/list` and not installing a workspace path policy, while the daemon/ACP path did. Its 13-pair frozen score improved one real task with zero regressions. Current `src/exec.rs` still constructs `ToolRegistry::with_config_and_search_runtime(&config, search_runtime)`, which defaults `ToolRegistryOptions.workspace_root` to None and registers unrooted workspace tools; the same function separately passes `exec_workspace_root` to `AgentLoop::new`. This **looks suspicious but is not proof** of the currently observable behavior. Original accepted red test `tests/exec_workspace_path_policy.rs` is absent from main, so the evidence is not continuous. Review current `src/main.rs` standalone run and `src/agent/loop.rs` construction before touching code.

## 3. Invariants

- `run` and `exec` without a valid current project root **refuse** rather than silently adopting a root from model arguments or unrelated process/global state.
- A valid headless root is supplied to **tool registry construction** and the existing broker path-policy authority, not just stored in a loop struct. Tool definitions, arguments and actual execution use one root.
- Explicit `workspace-write` and `full-host` effective policies are respected; an explicit Deny remains Deny. No hidden permission bypass, default daemon mutation, or ambient external worktree access.
- Preserve normal TUI/daemon/ACP behavior and existing shared-worktree writer fences; do not let a low-level registry constructor loosen default unrooted security.
- Independent verifier/test is checked in main; no reference to an unmerged commit substitutes for runtime evidence. Do not auto-merge a candidate.

## 4. Dependencies

None beyond existing CodeGG headless/ACP contracts and C003 archived evidence. Plan is ready independently. Coordinate with D002 if both change ACP tool events. No provider-backed run required to prove baseline capability.

## 5. Ordered work packages

A. **Historical-to-main diff + failing-first probe:** locate C003 candidate commit and changed paths/changelog from prior receipt; check whether cherry-picked, superseded, or never integrated. Add an independent test matching its original user-visible negative: local fake OpenAI-compatible provider advertises tool names, asks `read` and `write` in a confined temporary workspace; verify provider sees real rooted definitions, the broker approves within root under selected effective policy, and a sibling outside root is unavailable/denied. Run against unmodified current main first. Do not infer from API names.

B. **Minimal targeted patch if red:** thread the exact selected workspace root into `ToolRegistryOptions` and policy/writer-fence creation through an existing trusted production constructor or narrow explicit options path. Cover both `codegg exec` and `codegg run`; avoid duplicate tool registration, ambient fallback or an unrelated refactor. If already green, document a no-op closure with source SHA/proof instead. Original candidate may be used for guidance, not blindly cherry-picked.

C. **Mainline promotion contract:** ordinary reviewed PR/merge handoff, not controller auto-merge. Attach candidate diff provenance, accepted/negative test receipts, exact source SHA, and raw task outcome to closure; if a fix is deliberately not promoted, record as a blocker rather than claiming main is fixed.

D. **Regression and security sweep:** test missing cwd, symlink/relative/out-of-workspace path, nested worktree, config override, explicit Deny, both approval profiles, no default daemon; ensure rootless tool definitions do not become visible by force.

## 6. Failure, concurrency, restart

If the cwd vanishes after startup or canonical root changes during a turn, fail the affected tool/broker request, never fall back to global cwd. Each headless invocation owns its workspace snapshot; concurrent test invocations use distinct private temp trees and per-run identities. No process-owning child should survive cancellation.

## 7. Compatibility / migration

No production DB or protocol migration. Headless `exec` and `run` retain their existing CLI flags/output schema; any intentional change in missing-project behavior is documented and tested. Never alter semantics of ACP policy flags or TUI roots.

## 8. Required verification

```bash
cargo test -p codegg --test exec_workspace_path_policy
cargo test -p codegg --test acp_stdio
cargo test -p codegg --lib agent
python3 -m unittest discover -s dev/harness_lab/tests -v
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```
Adjust the test path to the current integration test layout, declare `no test executed` if the new test was not yet created, and report exact test count/exit. Use only a fixture provider for CI; a live model pilot is optional and separately budget-authorized.

## 9. Acceptance and stop conditions

Close positively only when actual current-main-derived binary proves `read/write/edit/glob/grep/list` availability and correct broker execution for `run` and `exec`, while unrooted/denied requests remain rejected, with green regression tests and applicable exact-head CI. A green ACP test **alone** is insufficient. No-op closure requires the same functional proof.

Stop if repair requires changing ownership of workspace authority or weakening writer/permission policy; raise an ADR instead. If unrelated current CI is failing, record the independent blocker and focused passing evidence without claiming an entirely green head.

## 10. Closure / handoff

Write `plans/closure/harness-lab-continuation/001-status.md` using repo closure matrix convention: prior accepted candidate SHA vs main source SHA, red-before/green-after test, tool-definitions and actual execution proof, explicit policy/Deny matrix, code/test files, exact-run CI results, remaining issues, and status. Update continuation roadmap + registry in **the same closure pass**; preserve earlier C003 evidence and its original accepted status.
