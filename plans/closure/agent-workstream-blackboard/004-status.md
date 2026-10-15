# Agent Workstream Blackboard M004 — Closure Status

Status: conditionally closed

Source implementation plan: `plans/implementation/agent-workstream-blackboard/004-peer-observation-and-tools.md`

Source subsystem roadmap: `plans/subsystems/agent-workstream-blackboard-roadmap.md#milestones--deliverables`

Implementation commit: `8a214a58fb287390e9ed35690f6c72246a4c9058`.

## 1. Executive finding

M004 delivers bounded model-facing group coordination and push-only peer
observations. The existing `task` tool exposes `board`, `claim`,
`release_claim`, and `request_handoff`. Claim ownership remains governed by
M003 CAS; a handoff request appends provenance and does not transfer ownership.
Run-control authorization remains the only sibling control authority.

Child loops restore group membership from durable group state, filter events by
originating session and group membership, reduce them to fixed host-owned
fields, and append them as explicitly untrusted user data at provider
boundaries. Durable result evidence carries bounded relative paths and typed
validation kind/status values; it is delivered only when changed paths overlap
another member's active path claim. No peer event wakes an idle run.

The milestone is conditionally closed because the current safe-boundary
contract cannot guarantee delivery before every possible next mutation when a
hint arrives during an already-running provider request. Delivery occurs at
the next provider boundary. The prescribed deterministic fake Git/provider
end-to-end scenario has not been added, so exact sequencing and the unrelated
change filter lack full harness evidence. There is no reason to add sibling
messaging or mid-turn transcript mutation to remove this condition.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A. Host observations from canonical events and results | `src/scheduler/executors.rs`; `AgentRunEvidenceObserved` in `crates/codegg-core/src/bus/events.rs`; `src/agent/follow_up.rs` | partial | Durable result commit, bounded relative paths, and typed validation kind/status are observed. Standalone test-job facts and progress detail explicitly degrade to unavailable; natural-language summaries are not treated as test proof. |
| B. Bounded, relevant, deduplicated feed | `peer_observation_message`; `path_claim_overlaps`; claim target map and seen-event set in `src/agent/follow_up.rs` | partial | 16 items, 8 KiB, fixed allowlist, coalesced duplicate revisions, and path-claim overlap filtering. No dedicated multi-agent fake-provider/Git sequencing test proves the requested exact one-observation timing or all unrelated-change cases. |
| C. Model tool and scope | `src/tool/task.rs`; task tool schema test | pass | Four coordination actions; board is group-authorized and marks cursor as `snapshot_only`; claim/release are CAS-fenced; handoff request has no ownership effect; legacy spawn/status and direct control remain separate. |
| D. Safe-boundary delivery and recovery | `src/agent/loop.rs`, `src/agent/worker.rs`, `src/agent/follow_up.rs`; durable `groups_for_member` lookup | partial | Push-only; run startup and broadcast lag produce a continuity gap; no idle wake. A hint arriving during a provider request can be consumed only at the following provider boundary, after any tool batch already returned by that request. |
| E. Projection/TUI | `crates/codegg-protocol/src/projection/snapshot.rs`; `src/tui/components/sidebar.rs`; `architecture/projection.md` | pass | Group owner/status, claims, dependencies, and existing run result/worktree/attention summaries are display-only. Explicit board-refresh marker preserves claims across generic lifecycle updates and clears them on an empty complete refresh. |
| Read-only boundary, no new authority | `src/core/mod.rs`; `src/tool/task.rs`; `architecture/agent.md` | pass | Internal claim/evidence events are not exposed as client Core events. Board reads do not change filesystem, Git, scheduler, sandbox, or sibling-control authority. |
| Cross-project and hostile-output isolation | Session/group filters and untrusted user-message encoding in `src/agent/follow_up.rs` | pass with test limitation | No raw tool output, prompts, credentials, terminal prose, or free-form progress are injected. Dedicated cross-project bus integration coverage remains part of the missing end-to-end harness. |
| Bounds and single-agent path | `MAX_PEER_OBSERVATION_ITEMS`, `MAX_PEER_OBSERVATION_BYTES`; no run ID means no receiver/injection | pass | Per-boundary drain, item count, bytes, dedupe-set cap, and first-party run scope are bounded. |

## 3. Verification executed

Native arm64, Rust 1.89, offline Cargo, `CARGO_BUILD_JOBS=2`, target directory
`/tmp/codegg-blackboard-arm64-target`:

- `cargo fmt --all` — passed.
- `cargo check --offline -p codegg --all-targets` — passed.
- `cargo test --offline -p codegg --lib agent::follow_up::peer_observation_tests` — 3 passed.
- `cargo test --offline -p codegg --lib tool::task::tests` — 4 passed.
- `cargo test --offline -p codegg-protocol projection::snapshot::tests::group_lifecycle_refresh_preserves_board_until_complete_claim_refresh` — 1 passed.
- `cargo test --offline --test workstream_claim_m003` — 5 passed (also verifies handoff-request persistence and unchanged ownership).
- `bash scripts/verify.sh quick` — passed after implementation, including workspace all-target check and required guards.
- `cargo clippy --offline --workspace --lib --locked -- -D warnings` — passed.
- `cargo clippy --offline --workspace --all-targets --locked -- -D warnings` — did not pass; stopped on the pre-existing `clippy::explicit_auto_deref` warning at `crates/codegg-providers/src/provider_core.rs:2941` in the provider test target. No M004 source warning was reported before that failure.

Hosted CI, non-macOS targets, and live-provider tests were not run.

## 4. Limitations and residual evidence

| Severity | Finding | Impact | Follow-up |
|---|---|---|---|
| medium | Peer hints are consumed at provider boundaries; a hint arriving during an in-flight response is not guaranteed to precede a tool call already returned by that response. | M004 acceptance item 1's strict before-next-modification timing is not proven for every active-turn interleaving. | Preserve safe-boundary transcript ownership. A future accepted design may add a typed pre-mutation boundary check; do not mutate in-flight provider context or wake idle runs. |
| low | Standalone scheduler test-job events and natural-language progress details are not peer evidence sources. | Board and peer hint explicitly show unavailable detail instead of inferring success. | Add a typed, authorized job observation only with a deterministic harness and source provenance. |
| low | No scripted two-worktree/fake-Git/provider end-to-end test exercises exact model-visible sequence, reconnect, queue overflow, and unrelated path filtering together. | Several acceptance rows have implementation evidence but not integrated harness proof. | Add this harness before claiming strict M004 closure. |
| low | Workspace all-target Clippy remains red on the unrelated provider test warning above. | Strict workspace Clippy signal is unavailable for the full test target. | Resolve in provider-owned scope; M004 library targets passed strict Clippy. |

## 5. Downstream unblocking audit

- **M005 group worktree placement:** remains blocked. M004 is conditionally
  closed, while M005 requires strict M004 closure, explicit acceptance of
  proposed ADR-0032, and verified containment of every writer path. ADR-0032
  remains proposed; this closure does not authorize shared checkout mutation.
- **M006 session/worktree launch ergonomics:** remains blocked on M005.
- **M007 build-resource coordination:** production implementation remains
  blocked on M005. Its plan explicitly allows the scheduler/build resource
  interface design to proceed earlier; this eligible design-only work will be
  audited separately and does not authorize cache sharing or placement.
- **M008 trajectory/economics:** remains blocked on M004/M006/M007.
- No other implementation plan in this roadmap became ready.

## 6. Registry and documentation updates

- The M004 implementation plan links this closure and records its conditional
  status.
- `plans/registry.md` records M004 as conditionally closed and retains M005's
  named ADR and writer-containment gates.
- `plans/subsystems/agent-workstream-blackboard-roadmap.md` records M004's
  conditional closure and the M007 design-only eligibility.
- `architecture/agent.md`, `architecture/bus.md`,
  `architecture/projection.md`, `.opencode/skills/agent/SKILL.md`, and
  `.opencode/skills/bus-projection/SKILL.md` describe the implementation and
  its trust, cursor, replay, and availability boundaries.
