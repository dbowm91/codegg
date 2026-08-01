# Agent Runtime, Model Adaptation, and ACP Milestone 013 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/agent-runtime-model-adaptation-acp/013-specialized-runtime-finalization-and-research-coordination.md`
Source subsystem roadmap: `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md#milestone-013--specialized-runtime-finalization-and-research-coordination`
Repository baseline reviewed: `d91ccea`
Implementation commits: `d91ccea — Implement specialized runtime finalization`

## 1. Executive finding

Milestone 013 is strictly closed. Specialized security and research turns now
remain ordinary agent-loop executions, while host-owned preparation,
coordination, and finalization establish the correctness boundary. Completion
is published only after local typed-output validation succeeds.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Bounded terminal output without private reasoning | `AgentLoop::terminal_output` collects public text, stop reason, usage, and tool-event count while ignoring reasoning deltas. |
| Security finalization | `DefaultTurnRuntime` prepares `SecurityEvidenceBundle`; the spawned turn parses bounded JSON and applies `security::runtime::validate_report`. Unsupported findings become evidence gaps. |
| Research child coordination | `agent::specialized_runtime::coordinate_research` uses `SubAgentPool::send_and_wait`, fixed `general` evidence children, explicit read-only denied tools, workspace paths, depth, timeout, and tool-call limits. |
| Typed child evidence | Child output must deserialize as `ResearchEvidenceReport` with the planned task ID; malformed output becomes an explicit limitation. |
| Evidence ledger | Source identity normalization/deduplication, bounded evidence/claims/limitations, reference filtering, and deterministic normalized-claim conflict capture are implemented in `aggregate_research`. |
| Root report validation | Research synthesis output is parsed as `ResearchReport`, checked by local `validate_report`, checked against the validated ledger, and gated by request-kind minimum evidence. |
| Ordinary runtime ownership | No provider, tool, permission, scheduler, or second orchestration implementation was introduced. |

## 3. Production implementation evidence

- Security and research preparation remain in `DefaultTurnRuntime` and feed
  the ordinary prompt/compiler path.
- Research children use the existing sub-agent pool and are bounded to the
  plan's maximum three tasks, 120-second child timeout, and 24 tool calls.
- The finalizer receives only `AgentLoopTerminalOutput`; provider reasoning is
  not available at the finalization boundary.
- Finalizer failures use the existing `TurnFailed`/agent error path and do not
  emit successful completion.
- Architecture documentation records the specialized lifecycle and evidence
  authority in `architecture/agent.md`, `architecture/security.md`, and
  `architecture/research.md`.

## 4. Verification executed

Local verification completed:

- `cargo fmt --all -- --check` — pass.
- `cargo check -p codegg --all-targets` — pass, existing warnings only.
- `cargo test -p codegg security::runtime` — pass.
- `cargo test -p codegg research::runtime` — pass.
- `cargo test -p codegg specialized_runtime` — pass.
- `python3 scripts/check_scheduler_bypass.py` — pass.
- `python3 scripts/check_execution_ownership.py` — pass.
- `bash scripts/check_projection_disclosure.sh` — pass.
- `git diff --check` — pass before implementation commit.

No live search, external scanner, paid provider, or network-dependent test was
required by the milestone.

## 5. Invariant review

Provider structured-output support remains advisory. Security findings require
prepared target/evidence support, and research claims cannot cite unknown
ledger sources. Children cannot delegate, mutate, or widen their workspace
scope through the specialized coordinator. Reports and prompt ledger content
are bounded; private reasoning and full retrieved bodies are not serialized by
the new seam.

## 6. Failure and recovery review

Malformed or oversized security/research JSON fails finalization. Child
timeouts, worker failures, and malformed child reports become explicit research
limitations; direct and multi-source requests fail minimum-evidence gating
when required evidence is absent. Existing loop cancellation and pool
shutdown paths remain authoritative, with bounded child timeouts preventing an
indefinite coordination wait. No partial report is marked complete.

## 7. Migration and compatibility review

`AgentLoop::run` retains its existing event-vector return contract. The typed
terminal collector is additive and internal. Generic security/research tools,
agent names, protocol DTOs, and durable storage are unchanged.

## 8. Security review

The specialized path is read-only. Child requests deny mutation, delegation,
and editing tools and pass the parent workspace as the only allowed path.
Untrusted provider output is parsed as data and cannot alter runtime authority.
Malformed citations and unsupported findings cannot become confirmed output.

## 9. Documentation and operations

The agent, security, and research architecture documents now describe the
prepare/coordinate/finalize boundary and validation authority. The plan and
corrective addendum status lines, registry, and this closure record are
updated together with the implementation evidence.

## 10. Unresolved findings

- Low: research child quality still depends on the configured provider
  producing the requested typed JSON; noncompliance is explicitly rejected,
  but no provider-specific repair loop was added, consistent with scope.
- Low: cancellation during the short pre-loop coordination window is bounded
  by the existing child timeout; the ordinary loop's cancellation remains
  authoritative after the turn handle is returned.

No critical, high, or medium finding remains. These low-severity limitations
do not weaken the strict acceptance criteria or block the next milestone.

## 11. Roadmap disposition

Milestone 013 is closed. Milestone 014 is dependency-ready because M013 is its
only hard predecessor and the prompt/evidence ledger contract is now stable.
Milestones 015–017 remain blocked: they still require M014, then M015, and the
full M012–M016 closure set respectively.

## 12. Registry updates

- M013 moved from `closing` to `closed` and is recorded under recently closed
  work in `plans/registry.md`.
- M014 moved from `blocked` to `ready` in the same closure commit because its
  only named dependency, strict M013 closure, is satisfied.
- M015, M016, and M017 remain blocked with their existing predecessor
  descriptions.
- No new corrective plan is required; the two low-severity limitations are
  bounded and explicitly documented above.
