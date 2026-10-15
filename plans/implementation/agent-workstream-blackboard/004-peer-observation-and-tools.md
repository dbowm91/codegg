# Agent Workstream Blackboard M004 — Peer Activity Feed and Model-Facing Coordination

Status: conditionally closed — see `plans/closure/agent-workstream-blackboard/004-status.md`

Planning baseline: de49884f3fdc6d528620f2a2645694b9b19fc087 (2026-10-15)
Source: plans/subsystems/agent-workstream-blackboard-roadmap.md M004
ADRs: ADR-0001/0002/0004 accepted; ADR-0032 proposed (no shared mutation here).
Canonical: plans/000-long-term-specification.md §§16–17, 26–29; plans/001-terminology-and-domain-model.md §§7, 12–13.
Primary class: capability. M002 observed state and M003 claims are prerequisites.

## Objective

Make authorized subagents aware of relevant siblings without copying conversations or depending on parent-authored polling. Provide a typed read/claim/handoff-request interface and a bounded, automatically delivered peer-observation block at safe agent-loop boundaries when meaningful facts change. The feed is a statement of host observations, never a sibling instruction channel.

## Existing evidence

src/tool/task.rs contains spawn/spawn_many/group/status/message; src/agent/run_control.rs only allows owner-to-child/direct-parent control; worker/turn runtimes have safe follow-up boundaries. Canonical result facts are available in AgentRunResult/WorktreeService and scheduler Job/RunStore. Existing status_group is coarse; no model-facing workstream peer feed exists.

## Work packages

A. Host observation pipeline. Subscribe to authoritative run/group terminal and progress transitions, Git-derived result commits/changed paths, meaningful test/build status and M003 claim/handoff events. Attribute each fact to source RunId/JobId/WorktreeId, time/revision, observation kind and trust class. Do not parse natural-language assertions as verified commits, success, test status or file ownership. Unsupported observation types degrade explicitly.

B. Bounded relevance. Each child sees same-group authorized peer summaries, highest priority for dependencies, overlapping claims and interface changes. Add a stable revision cursor, coalescing, ordering and caps (bytes/items/time window). Avoid broadcasting identical 'still running' observations; do not assemble unbounded transcripts, tool logs, stack traces, prompts or credentials. No peer status scan loop.

C. Model tool. Prefer extending the existing task tool with 2–4 concise coordination actions (e.g., board, claim, release, request_handoff) rather than many top-level tools or an unconstrained API. Strict input schema, scope checks and clear usage contract; only the group owner can effect ownership reallocation. Keep legacy spawn/status unchanged. Prevent a sibling from using group visibility to message, cancel, interrupt or manage another run.

D. Safe-boundary context delivery. Append the bounded peer facts snapshot to the model's next safe agent-loop boundary, coalescing progress after context compaction and resumption. Progress alone never starts a new model turn after an idle or ended session without existing explicit continuation authority (ADR-0031). Active-turn notifications are push hints; canonical state is re-read by revision when consumed.

E. Projection/TUI. Show owner, claims, blocked dependencies, observed Git/test status and attention reasons; no frontend authority. Add small TUI group-inspection affordance, but do not invent an IDE or full sibling-chat UI.

## Acceptance matrix

1. A and B work in distinct worktrees: after A changes a shared API and commits, B receives one evidence-backed relevant observation before its next modification; unrelated changes are filtered.
2. A and B share only read access: no filesystem mutation authority appears because of blackboard reads.
3. Duplicate progress, reconnect, compaction, cancellation, bounded queue overflow and daemon restart do not produce conflicting or repeated instruction blocks; explicit gap notice when sequence continuity unavailable.
4. Two unrelated projects/team principals remain isolated; raw tool output with prompt-injection instructions is tagged as untrusted and never promoted to system/developer instructions.
5. Large logs/secrets, 16-member groups and bursty tests remain within serialization/token/memory caps.
6. A sibling cannot forge completion, steal claims, or steer peers; parent control remains exact direct-run/turn ownership.
7. Baseline single-agent turn sends no peer observation payload by default; no per-tool latency penalty when no group active.

## Verification and closure

Use scripted providers and deterministic fake Git/job emitters to test exact model-visible bytes, budget, order and source provenance. Extend task tool, run-control, session-projection, compaction and authorization suites; cargo fmt; core/protocol/scheduler/ownership guards; scripts/verify.sh quick; workspace Clippy and optionally existing hosted CI. No mandatory live-provider benchmark in CI.

Stop on raw command/tool-output injection into authoritative model instructions, project leak, duplicate model wake, or need for direct sibling messages. Record full matrix and failure modes in plans/closure/agent-workstream-blackboard/004-status.md. M005 remains blocked on M001 and accepted ADR-0032 even after this milestone closes.
