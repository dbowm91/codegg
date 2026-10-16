# Agent Workstream Blackboard M004 Strict Closure — Corrective 009

Status: closed (strict M004 timing invariant; low-evidence limitations recorded)

Source corrective plan: `plans/implementation/agent-workstream-blackboard/009-peer-observation-before-mutation-corrective.md`

Original M004 plan: `plans/implementation/agent-workstream-blackboard/004-peer-observation-and-tools.md`

Historical conditional closure: `plans/closure/agent-workstream-blackboard/004-status.md` (preserved unchanged)

Implementation commit: `8316418a670922d17d98de4a7ed3e9b572bec825`.

Closure evidence commit: recorded by Git history for this file.

## 1. Executive finding

M004 is strictly closed for its peer-observation-before-next-action invariant.
The loop now drains bounded peer state after a complete provider response and
before dispatching its tool batch. A relevant observation or continuity gap
refuses every action in that batch. The assistant tool-call/results pair is
retained, then the untrusted observation is appended for the next provider
request. The in-flight provider transcript is never mutated, no sibling is
woken, and this fence adds no writer serialization or execution authority.

The original 004 closure remains an immutable historical record of the
conditional status and its then-unresolved medium timing finding. This
corrective is the current strict status authority. ADR-0032 is still proposed
at this closure commit; acceptance is a separate subsequent decision. M005
remains blocked independently on writer-path containment.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Relevant same-group evidence published during a provider request is visible before the returned mutation batch can execute | `src/agent/follow_up.rs::fence_tool_batch_on_peer_update`; `src/agent/loop.rs`; `tests/agent_loop_harness.rs::peer_evidence_arriving_during_provider_stream_fences_returned_tool_batch` | pass | Fake provider publishes a host-owned result event while streaming. An overlapping peer path claim makes the result relevant. Both write calls are refused; the following request contains commit/path evidence after both matching tool results. |
| Unrelated changes do not notify or fence | `tests/agent_loop_harness.rs::unrelated_peer_change_does_not_fence_or_notify_member`; `src/agent/follow_up.rs::path_claim_overlaps` | pass | The provider returns two writes; a changed path outside the active claim leaves both enabled and is omitted from peer evidence. |
| Cross-session/project observations cannot leak | Both scripted harness cases publish a same-run event under a distinct session identifier; recipient request asserts the other project commit is absent | pass | Session scope is checked before evidence reduction. |
| Refusal preserves complete tool transcript and requires reconsideration | Relevant-event harness test; `Message::Tool` IDs and observation-order assertions | pass | Both tool results remain paired with their calls and precede the peer observation in the next request. No refused action is automatically replayed. |
| Continuity gaps fail closed | `inject_peer_observations` lag/restart-gap handling and corrective fence; original M004 closure §2 and existing peer-observation unit coverage | pass | Startup/reconnect gaps are emitted even without a payload; a gap discovered at the post-provider fence refuses the batch and directs the model to refresh `task action=board`. |
| Bounded, allowlisted, untrusted payload | `peer_observation_message`; `agent::follow_up::peer_observation_tests` (3 tests) | pass | Item/byte bounds, oversized-item truncation, path-boundary relevance and user-message encoding are retained. No raw tool output or prose is promoted to privileged instructions. |
| Coordination actions retain authorization and CAS boundaries | `tool::task::tests` (4 tests); `workstream_claim_m003` (5 tests) | pass | Board/claim/release/handoff actions remain group-scoped; handoff requests do not transfer claims; stale CAS, membership, terminal-owner and restart cases remain covered. |
| Projection refresh retains complete workstream board state | `projection::snapshot::tests::group_lifecycle_refresh_preserves_board_until_complete_claim_refresh` | pass | Generic lifecycle refresh cannot erase claims; complete refresh remains authoritative. |
| No-group/single-agent and idle behavior | `fence_tool_batch_on_peer_update` returns before draining when no peer group exists; existing `test_no_follow_up_latency`; original M004 closure §2 | pass | No receiver or per-tool wait is added for non-group execution; no event starts an idle model turn. |

## 3. Production implementation evidence

Implementation commit `8316418a670922d17d98de4a7ed3e9b572bec825` adds the
post-provider peer-event fence in `src/agent/follow_up.rs`, applies it in both
the primary and autonomous follow-up tool loops in `src/agent/loop.rs` and
`src/agent/follow_up.rs`, and adds deterministic multi-call provider coverage
in `tests/agent_loop_harness.rs`. `architecture/agent.md` and
`.opencode/skills/agent/SKILL.md` now state the fence contract and its boundary:
it does not serialize writers.

No schema, protocol, migration, or new execution authority was added.

## 4. Verification

Native arm64 macOS, Rust 1.89, offline Cargo, `CARGO_BUILD_JOBS=2`, target
directory `/tmp/codegg-blackboard-arm64-target`:

- `cargo fmt --all -- --check` — passed.
- `cargo test --offline -p codegg --test agent_loop_harness` — 55 passed.
- `cargo test --offline -p codegg --lib agent::follow_up::peer_observation_tests` — 3 passed.
- `cargo test --offline -p codegg --lib tool::task::tests` — 4 passed.
- `cargo test --offline --test workstream_claim_m003` — 5 passed.
- `cargo test --offline -p codegg-protocol projection::snapshot::tests::group_lifecycle_refresh_preserves_board_until_complete_claim_refresh` — 1 passed.
- `bash scripts/verify.sh quick` — passed, including workspace `cargo check --all-targets --locked` and core/client/desktop boundaries, sandbox, ADR integrity, execution ownership, scheduler bypass, route disposition, audit, config merge, provider, and Eggwork guards.
- `cargo clippy --offline --workspace --lib --locked -- -D warnings` — passed.
- `cargo clippy --offline --workspace --all-targets --locked -- -D warnings` — blocked by the existing `clippy::explicit_auto_deref` warning in `crates/codegg-providers/src/provider_core.rs:2941` (`*secret`); no M004 target warning was reported.
- Hosted CI and non-macOS runtime qualification — not run.

## 5. Invariant review

- Provider request messages remain immutable during provider execution.
- Once the post-response drain discovers relevant peer evidence or lost
  continuity, no action from that response is dispatched. Every action gets a
  paired typed denied result before the observation is appended.
- Delivery remains bounded, allowlisted, deduplicated, same-session and
  same-group scoped, and explicitly untrusted.
- The fence has no effect when the run has no peer group. It does not wake
  idle runs, steer siblings, or change ToolBroker, permission, scheduler,
  sandbox, filesystem, or Git authority.
- The fence provides no writer serialization; ADR-0002 isolation remains the
  active mutation contract until M005 qualifies another placement mode.

## 6. Failure and recovery review

- A broadcast lag or fresh run receiver creates an explicit continuity gap.
  The gap survives an empty observation payload and fences a returned tool
  batch until the model can refresh canonical board state.
- A refused tool call is not replayed automatically. Its result remains paired
  with the assistant call; a subsequent provider request receives the
  observation and makes a new decision.
- Provider errors before a complete response produce no returned tool batch;
  this change does not alter existing cancellation or provider error recovery.
- Same-group state under a different session/project is excluded by the
  session check and asserted in the scripted harness.

## 7. Migration and compatibility review

- No migration, persisted event format, public protocol shape, or storage
  layout changed.
- Existing persisted group/claim records and ADR-0002 isolated-child defaults
  are unchanged.
- The new fence is internal to AgentLoop and retains the existing ToolBroker
  dispatch path. Non-group and single-agent turns do not acquire a receiver or
  per-tool wait.

## 8. Security review

- Peer payload remains host-reduced, bounded, allowlisted, and placed in a user
  message marked as untrusted. Raw tool output, commands, prompts, credentials,
  and arbitrary peer prose are not elevated to system/developer instructions.
- Session and group membership plus active path-claim overlap constrain the
  evidence. Wrong-session evidence is excluded by integration test.
- Existing M003 CAS, handoff, membership and run-control authority remain
  unchanged; no sibling messaging or control capability was added.
- The correction does not weaken approval, sandbox or Git policy and does not
  authorize shared checkout mutation.

## 9. Documentation and operations

- `architecture/agent.md` and `.opencode/skills/agent/SKILL.md` describe the
  post-provider fence, gap behavior and writer-serialization boundary.
- The M004 and M009 plan records, roadmap, closure registry and dependency
  gates link to the strict closure. Original closure 004 remains unchanged.
- Hosted CI and other-platform runtime checks were not run; no claim is made
  about those environments.

## 10. Unresolved findings

| Severity | Finding | Impact | Disposition |
|---|---|---|---|
| low | The deterministic acceptance harness uses two durable member IDs, a host-owned fake commit/result event and an overlapping path claim, but does not allocate two physical `WorktreeService` roots or create a Git commit on disk. | The peer-event ordering invariant and path relevance are exercised at the event boundary; filesystem placement and Git collection remain M005/host-service concerns. | No shared checkout or physical worktree behavior is claimed by M004. Add a WorktreeService-backed trajectory when a plan requires that substrate. |
| low | The full lifecycle matrix for 16 members, queue overflow, duplicate replay, cancellation and daemon restart is not re-run as one provider-backed trajectory here. | Existing unit/source/claim/projection coverage establishes the relevant bounded and recovery contracts separately. | Keep the existing individual tests and original 004 evidence; do not claim a single end-to-end lifecycle simulation. |
| low | Workspace all-target Clippy remains red on the provider test-target warning above. | Full test-target lint signal is not green. | Workspace library Clippy and quick verification passed; provider warning is outside this correction. |

None of these findings permit a stale in-flight provider action to execute after
the post-response fence has observed a relevant fact or continuity gap. They do
not falsify M004's closed timing invariant or authorize shared-checkout writes.

## 11. Roadmap disposition and downstream unblocking audit

- **M005 group worktree placement remains blocked.** M001 and strict M004 are
  now complete. At this closure point ADR-0032 remains proposed, and writer
  containment across file, Git, shell, terminal, plugin, LSP, tool-program and
  subprocess mutation paths is not proven. Even after ADR acceptance, this
  containment gate remains mandatory before M005 can become ready or shared
  mutation can be enabled.
- **M006 session/worktree launch ergonomics** remains blocked on M005.
- **M007 build-resource production coordination** remains blocked on M005.
  Its design-only interface candidate was already eligible and recorded in its
  plan; this closure does not authorize production resource or cache changes.
- **M008 trajectory/economics** remains blocked on M006 and M007, even though
  its M004 dependency is now satisfied.
- No additional implementation plan became ready. Registry and roadmap state
  are updated with this closure; downstream gates are unchanged except that
  strict M004 is discharged.

## 12. Registry updates

- M009 is closed by this record; original M004 closure 004 remains unchanged.
- M004 is marked strictly closed through corrective 009 in the roadmap and
  registry.
- ADR-0032 acceptance is deliberately recorded in a separate subsequent
  change, as requested. It will not remove M005's writer-containment gate.
