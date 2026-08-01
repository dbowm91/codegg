# Agent Runtime, Model Adaptation, and ACP Milestone 014 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/agent-runtime-model-adaptation-acp/014-canonical-prompt-and-context-plan-convergence.md`
Source subsystem roadmap: `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md#milestone-014--canonical-prompt-and-context-plan-convergence`
Repository baseline reviewed: `81b46de801137df605ce302dccff6f258c99fae1`
Implementation commits: `81b46de — Implement canonical prompt context convergence`

## 1. Executive finding

Milestone 014 is strictly closed. Root prompt assembly now collects runtime
context into typed, bounded prompt blocks before compilation. Descendant prompt
assembly uses the same compiler contract and resolved adapter identity.
Post-compilation root system-string mutation and duplicate plan-mode guidance
were removed. Context-plan cache identity receives the compiler fingerprint
directly.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Typed block contract | `PromptBlockKind`, source identity, cache class, required flag, bounded content, and content hash in `src/agent/prompt.rs`. |
| Deterministic ordering | Compiler sorts blocks by stable contract order; prompt unit test verifies equivalent inputs are identical. |
| Root convergence | `DefaultTurnRuntime` collects memory, goal, security, research, LSP, Git, assets, skills, and plan state before `PromptCompiler::compile`. |
| Single plan contract | Plan mode is emitted by the base compiler block set; no root post-compile plan append remains. |
| Identity convergence | Compiler fingerprint includes ordered block identities, asset snapshot/pin, execution workspace/session, and adapter fingerprint. `AgentLoop` passes it to `ContextPlan`. |
| Descendant convergence | Worker compilation uses the same typed compiler and resolved adapter fingerprint; explicit runtime execution/snapshot inputs are used by the primary root path when available. |
| Protocol chronology | Existing `ContextPlan` validation and apply path remain lossless and chronological. |
| Privacy/bounds | Prompt blocks are bounded; context diagnostics remain hash/count based; private reasoning is not copied into prompt blocks. |

## 3. Production implementation evidence

- `PromptCompiler` is the sole production effective-system-context entry point.
- Runtime collectors execute before compilation and pass typed blocks rather
  than mutating the flattened system string.
- `ContextPlan` uses the stored compiler fingerprint, with a compatibility
  fallback only for legacy callers that do not provide one.
- Duplicate block identities are surfaced in bounded compiler diagnostics.
- Remote instruction URLs continue to be excluded unless the asset owner has
  supplied resolved content.

## 4. Verification executed

Local verification on `81b46de`:

- `cargo fmt --all` — pass.
- `cargo check -p codegg --all-targets` — pass; existing warnings only.
- `cargo test -p codegg --lib agent::prompt -- --test-threads=4` — 22 passed.
- `cargo test -p codegg --lib context::plan -- --test-threads=4` — 2 passed.
- `cargo test --test context_plan_convergence -- --test-threads=4` — 4 passed.
- `cargo test --test agent_loop_harness -- --test-threads=4` — 40 passed.
- `cargo test --test subagent -- --test-threads=4` — 22 passed.
- `python3 scripts/check_daemon_cwd_usage.py` — pass.
- `python3 scripts/check_project_agent_pwd_inference.py` — pass.
- `python3 scripts/check_builtin_agents.py` — pass.
- `python3 scripts/generate_builtin_agents.py --check` — pass.
- `git diff --check` — pass.

No network-dependent or live-provider verification was required.

## 5. Invariant review

Provider message chronology and assistant/tool-result pairing remain governed
by `ContextPlan`. Capability names remain canonical and sorted before prompt
construction. Asset identity is immutable for the active root turn. Required
prompt blocks are not eligible for optional context omission. No process-global
cwd resolution was added.

## 6. Failure and recovery review

Compilation is pure after asynchronous collectors complete. Asset pin identity
is cloned before later awaits, preventing a mutex guard from crossing the
runtime future. Oversized blocks truncate only at UTF-8 boundaries and record a
bounded marker. Existing context-policy recovery and full-request restoration
remain authoritative.

## 7. Migration and compatibility review

Provider DTOs and durable storage are unchanged. Existing agent/config,
instruction, skill, and context-plan formats remain readable. Descendant
callers without asset/execution snapshots retain the same compiler path and
receive adapter identity where available; no cwd fallback was introduced.

## 8. Security review

Prompt diagnostics contain block metadata and hashes, not content bodies or
private reasoning. Remote URLs are not presented as loaded instructions.
Specialized evidence remains bounded and host-prepared. The change introduces
no new network, permission, tool, or workspace authority.

## 9. Documentation and operations

`architecture/agent.md` and `architecture/cache-aware-context.md` document the
typed block contract, pre-compilation collection, and direct compiler/cache
identity relationship.

## 10. Unresolved findings

- Low: some legacy compatibility callers still use flattened-text fallback
  identity when they do not provide a compiler fingerprint; production root
  and descendant paths now provide the canonical fingerprint.
- Low: the existing descendant request DTO does not yet carry a full immutable
  asset snapshot across every scheduler-created child; the shared compiler
  contract and explicit workspace restrictions remain in force. This is the
  named follow-up scope of Milestone 016, not a blocker for M014.

No critical, high, or medium finding remains in M014 scope.

## 11. Roadmap disposition

Milestone 014 is closed. Milestone 015 is dependency-ready because M014 is
strictly closed. Milestones 016 and 017 remain blocked by their named
predecessors and are not promoted early.

## 12. Registry updates

- M014 moved from dependency-ready to closed.
- M015 moved from blocked to dependency-ready in the same closure change.
- M016 and M017 remain blocked.
