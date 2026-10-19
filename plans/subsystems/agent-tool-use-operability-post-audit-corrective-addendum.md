# Agent Tool-Use Operability — Post-Audit Corrective Addendum

Status: closed; C001–C008 implementation and qualification milestones are closed.
Planning baseline: CodeGG `4c86bac2a608dd717be085247838fd2f37d81ab6` (main, 2026-10-19)
Evidence: 2026-10-19 agent tool-surface exercise (Agent Findings 2); source review of the same main revision. This is reported runtime evidence, not a reproduced CI result.

Long-term references:
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

Related ADRs and predecessor authority:
- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`
- `plans/adrs/ADR-0028-command-syntax-and-execution-authority.md`
- `plans/subsystems/tool-programs-roadmap.md` and `plans/subsystems/tool-programs-correctness-closure-addendum.md` (closed foundations; historical closure retained)
- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md` (closed predecessor)
- `plans/subsystems/search-eggsearch-pair-compatibility-post-closure-corrective-addendum.md` (v0.4.3 local-only private fixture passed)
- `plans/subsystems/eggsearch-evidence-workflows-roadmap.md` (M002 repository goals and M004 semantic projection remain independently owned)
- `plans/subsystems/agent-recovery-escalation-authority-corrective-addendum.md` (separate incident/stall policy owner)

## 1. Purpose and ownership boundary

Close observed *model-facing operational gaps* in existing CodeGG tool paths, without reopening accepted design or inventing new execution/search authority. Own the adapter and production-agent path from native tool-call, permission receipt, broker, tool implementation, and presentation into model-visible results. Changes stay in their existing code owners; this addendum owns cross-tool integration and evidence.

The agent exercise reported local repo-search misses and irrelevant remote results, path validation failures attributed to a supposed read prerequisite, duplicate grep prefixes, multi-active todo rejection, unconditional verify `--all-features`, unusable tool_program permission context, confusing task lookup and execution permit timeouts. Source review distinguished confirmed defects from hypotheses: `repo_search` already forwards `include_local`; `codesearch` omits it from the alias request; `grep`/patch have no read-history guard; todo restriction is configured; broker reconstitutes a context with missing authority fields; batch permit deadlines are bounded independently of tool run time.

## 2. Work classification

Invariants: verified accepted authority survives broker dispatch unchanged; missing authority always fails closed; workspace containment/SSRF policy never weakened; no untrusted content becomes instructions; session/subagent identities remain distinct and authorized; no queued tool silently executes after a terminal cancellation.

Capabilities: dependable local search for an attached workspace; authorized first-use grep/patch; usable scoped verification and task-status retrieval; clear, bounded failures.

Infrastructure: queue/admission diagnostics and a scripted provider-driven cross-boundary acceptance trajectory using existing test infrastructure.

Polish: precise schema and path diagnostics, deduplicated grep output, truthful todo semantics, compact search results.

## 3. Non-goals

No general-purpose interpreter for Tool Programs; no new Python imports/subprocess authority; no second local index or independent web backend; no release/package work, persistent search cache, new external CI lane, SBC compilation/startup benchmark, global scheduler, new authorization engine or reduction of sandbox/path/permission checks. Do not implement eggsearch workflow M002's semantic goal controls or M004's general result-projection rewrite here. Do not change separate agent progress-recovery shortcycle/equivalentresult policy in this corrective.

## 4. Current state and evidence boundaries

- `src/tool/repo_search.rs` forwards requests to runtime; `src/search_backend/eggsearch.rs::call_repo_search_structured` copies `include_local`. `tests/eggsearch_pair_compat.rs` proves offline private fixture for the pinned child; no proof the reported production workspace config was indexed.
- `src/tool/codesearch.rs::request_input` reconstructs only query/profile/max_results, dropping other submitted fields.
- `src/tool/util.rs::{validate_path,validate_target_path}` enforce workspace containment without tracking reads; `src/tool/apply_patch.rs` uses target validation for creates and existing validation for updates.
- `src/tool/grep.rs::push_rendered_line` includes path; outer execute renderer adds it again.
- `crates/codegg-core/src/task_state/mod.rs::replace_from_model` rejects multiple in-progress only when policy requires it; `model_profile/types.rs::explicit_todo` defaults to true.
- `src/tool/verify.rs::ResolvedVerification::resolve` unconditionally adds `--all-features` to lint, conflicting with `AGENTS.md` scoped guidance; `plans/registry.md` broad-verification example also needs reconciliation.
- `src/agent/tool_batch.rs::build_tool_execution_context` starts with accepted receipt but no workspace path-policy IDs; `src/tool/broker.rs::execution_context_for_attempt` initializes receipt fields to None; `src/tool/tool_program_context.rs::to_core_context` rejects missing fields.
- `src/tool/task.rs`: status/wait take `run_id` but get takes `task_id`; descriptions conflate status/get.
- `src/agent/tool_batch.rs` creates a per-batch semaphore, applies `tool_timeout_secs` to permit waits, and reports Timeout before execution. Reported parent/subagent-wide resource contention is a hypothesis, not established by this semaphore.

## 5. Target contract

A fresh permitted tool call must succeed or fail with a specific, truthful, bounded reason; neither tool output nor permission scope may be silently rewritten. Local-search coverage, remote dispatch and truncation must be distinguishable. Verified authorization must survive complete normal broker dispatch; a direct test of the tool struct is insufficient closure evidence. Tool admission queue lifetime and run lifetime must be observable separately. No change may imply a new security guarantee without direct proof.

## 6. Dependencies and ownership

C001 authority | independent; hard predecessor closed; security review of verified grant projection.
C002 search | closed after strict C008 closure; runtime session/subagent scope, owner/repo and alias behavior, output bounds, no-index/no-match and local-only integration evidence passed; separate Eggsearch Evidence M002/M004 semantic/projection work remains independently owned. Follow-up closure `plans/closure/agent-tool-use-operability-post-audit-corrective/002-follow-up-status.md`.
C003 file tools | independent; existing workspace-root contract.
C004 task state | independent; task/run contract and profile config already in-tree.
C005 verify | independent; existing Cargo verifier and project testing policy.
C006 batch admission | independent of C001–C005 for implementation; only batch-local bounded admission.
C007 qualification/closure | closed after strict C002 follow-up closure and AgentLoop qualification across normal-path tool surfaces, no-index, scoped verification, explicit multi-active todos, and concurrent use. See follow-up closure; historical blocker remains immutable.
C008 host-scoped shared Eggsearch | closed; authenticated private metadata carries the accepted canonical root through one CodeGG-managed stdio child and pinned Eggsearch revision; isolated bounded local views and real-child/AgentLoop A/B isolation are qualified. CodeGG authorizes; Eggsearch enforces selected root/file containment. A public multi-client authority protocol is NOT authorized. Closure `plans/closure/agent-tool-use-operability-post-audit-corrective/008-status.md`.

These six bounded owner-specific slices may proceed on separate worktrees. Avoid concurrent edits of `plans/registry.md`; each closure owns its own status change with a lease/rebase. If another ongoing roadmap modifies a touched file, coordinate by path and retain its contract.

## 7. Milestones

| Milestone | Primary class | Plan | Status | Boundary and exit gate |
|---|---|---|---|---|
| C001 | invariant | `plans/implementation/agent-tool-use-operability-post-audit-corrective/001-broker-accepted-authority-projection.md` | closed | Real native Tool Program foreground/background trajectory and pre-job denial matrix; closure `plans/closure/agent-tool-use-operability-post-audit-corrective/001-status.md`. |
| C002 | capability | `plans/implementation/agent-tool-use-operability-post-audit-corrective/002-live-workspace-repository-search.md` | closed | Follow-up closure `plans/closure/agent-tool-use-operability-post-audit-corrective/002-follow-up-status.md`; historical blocker `002-status.md` remains unchanged. C007 was unblocked and is now strictly closed. |
| C003 | capability | `plans/implementation/agent-tool-use-operability-post-audit-corrective/003-grep-and-patch-first-use-contract.md` | closed | Real AgentLoop create→grep path and tool contract matrix; closure `plans/closure/agent-tool-use-operability-post-audit-corrective/003-status.md`. |
| C004 | polish | `plans/implementation/agent-tool-use-operability-post-audit-corrective/004-task-and-todo-status-contracts.md` | closed | Model-visible TaskTool status/get/group and cross-session denial; closure `plans/closure/agent-tool-use-operability-post-audit-corrective/004-status.md`. |
| C005 | capability | `plans/implementation/agent-tool-use-operability-post-audit-corrective/005-project-compatible-verify-defaults.md` | closed | Typed bounded Cargo scope, feature and CWD tests; closure `plans/closure/agent-tool-use-operability-post-audit-corrective/005-status.md`. |
| C006 | invariant | `plans/implementation/agent-tool-use-operability-post-audit-corrective/006-batch-permit-admission-liveness.md` | closed | Parent/subagent overlap, admission deadline race, and admitted native timeout with partial output; closure `plans/closure/agent-tool-use-operability-post-audit-corrective/006-status.md`. |
| C007 | capability | `plans/implementation/agent-tool-use-operability-post-audit-corrective/007-real-agent-tool-trajectory-qualification.md` | closed | Follow-up closure `plans/closure/agent-tool-use-operability-post-audit-corrective/007-follow-up-status.md`; production AgentLoop trajectories, pinned child isolation, no-index, authority, task, Tool Program, verification, and contention evidence passed. |
| C008 | infrastructure | `plans/implementation/agent-tool-use-operability-post-audit-corrective/008-shared-eggsearch-host-scoped-local-retrieval.md` | closed | Closure `plans/closure/agent-tool-use-operability-post-audit-corrective/008-status.md`; one shared Eggsearch child with authenticated CodeGG workspace scope, isolated bounded local views, real-child and AgentLoop A/B/A privacy proof, and quick verification. C002 and C007 subsequently closed; no remaining dependency in this corrective line. |

## 8. Cross-cutting requirements

Storage: no migration unless an evidence-backed defect establishes necessity; stop before introducing new persisted authority state.
Protocol: retain existing model-visible names unless correcting broken schema; additive fields and explicit compatibility notes where necessary.
Security: authorization receipts and path-policy IDs must derive from verified authority, never copied from model arguments. Symlink/traversal and network routing limits remain. No auto-retry of non-idempotent calls.
Concurrency: queue-wait separate from execution deadlines; bounded per-batch resources and terminal cancellation. Do not make global scheduler fairness claims based on the per-batch semaphore.
Observability: bounded IDs/reason codes and counts, no raw paths outside authorized diagnostic scope, secrets or complete tool outputs in telemetry.
Resource use: preserve caps; favor ranked selection and coverage metadata rather than enlarging context windows.
Documentation: update `architecture/{tool,agent,search_backend,permission}.md`, `docs/tools.md`, `AGENTS.md` only where affected; historical closure immutable.

## 9. Verification strategy

Focused unit tests for each owner; negative path/authority/permission tests; real pinned eggsearch *offline private-child* pair test; scripted agent/broker end-to-end trajectories with deterministic results; single `scripts/verify.sh quick` per coherent milestone and a final integrative pass; existing hosted CI only where closure demands. Never equate direct tool-unit tests with an actual agent successful call.

## 10. Risks and decision points

Production local-index root/config may be missing despite correct flag forwarding; report this distinctly rather than claiming upstream broken. C008 selects a host-authorized request-local scope and retains one daemon-owned Eggsearch child. Raw MCP callers cannot mint scope by injecting a hidden JSON field; the transport must be trusted. If independently authenticated or remote clients are required, stop for an ADR rather than accidentally exposing a local filesystem path selector. Changing accepted authority, task identity, scheduler ownership or public protocol requires an ADR and explicit approval; no such change is authorized here. `--all-features` advice currently conflicts between AGENTS and registry and must be reconciled with actual scripts rather than propagating either uncritically. Tool Program's missing workspace policy fields require a trusted source or explicit stop, not synthesized IDs.

## 11. Completion definition

C001–C008 are strictly closed with their implementation and integration evidence recorded in closure files. C008 unblocked C002; strict C002 closure then unblocked C007. No registered downstream plan became newly eligible, and no severity-high defect remains in the addressed surfaces. History and separate eggsearch/recovery roadmaps stay intact.

## 12. Milestone status

| Milestone | Status | Closure | Hard blockers |
|---|---|---|---|
| C001 | closed | `plans/closure/agent-tool-use-operability-post-audit-corrective/001-status.md` | None |
| C002 | closed | `plans/closure/agent-tool-use-operability-post-audit-corrective/002-follow-up-status.md` | C008 host scope, runtime session/subagent binding, alias/output bounds, local-only/no-index/no-match and integration disposition qualified; historical 002 record stays unchanged |
| C003 | closed | `plans/closure/agent-tool-use-operability-post-audit-corrective/003-status.md` | None |
| C004 | closed | `plans/closure/agent-tool-use-operability-post-audit-corrective/004-status.md` | None |
| C005 | closed | `plans/closure/agent-tool-use-operability-post-audit-corrective/005-status.md` | None |
| C006 | closed | `plans/closure/agent-tool-use-operability-post-audit-corrective/006-status.md` | None |
| C007 | closed | `plans/closure/agent-tool-use-operability-post-audit-corrective/007-follow-up-status.md` | None; historical 007 blocker record stays unchanged |
| C008 | closed | `plans/closure/agent-tool-use-operability-post-audit-corrective/008-status.md` | None; C002 and then C007 were unblocked and strictly closed |
