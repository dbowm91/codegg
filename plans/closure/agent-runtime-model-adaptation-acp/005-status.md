# Agent Runtime, Model Adaptation, and ACP Milestone 005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-runtime-model-adaptation-acp/005-specialized-research-runtime.md`

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-005--specialized-research-runtime`

Repository baseline reviewed: `672479726f1c79bbc931d70f084cd1649e8b2ed4`

Implementation commits:

- `e3db48c` — specialized research runtime, explicit service ownership,
  bounded evidence contracts, read-only research assets, and architecture
  documentation

## 1. Executive finding

Milestone 005 is closed. Resolved `runtime_kind = research` now selects a
host-prepared bounded plan before the ordinary agent loop. Quick lookups avoid
child creation; direct repository/spec questions receive one investigator
scope; multi-source questions receive at most two source scouts and one claim
verifier. The existing research service remains the evidence/synthesis engine,
but daemon construction is rooted in explicit workspace execution context.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Specialized runtime dispatch | `DefaultTurnRuntime` builds a research plan and requests the typed research response schema for `Research` | pass |
| Quick/direct/multi-source classification | `src/research/runtime.rs::classify` and focused tests | pass |
| Bounded, non-overlapping decomposition | `build_plan` caps the plan at three typed roles and tests unique task identities | pass |
| Explicit service construction | `ToolRegistry::with_options` uses `workspace_root` to construct `ResearchService`; cwd fallback is isolated to compatibility/test construction | pass |
| Structured evidence and reports | `ResearchEvidenceReport`, `EvidenceRecord`, `ClaimRecord`, `ClaimConflict`, and `ResearchReport` are serde-serializable and bounded | pass |
| Source normalization/deduplication | `normalize_source_identity` removes URL fragments/trailing slashes and `deduplicate_sources` caps output | pass |
| Citation validation | `validate_report` rejects empty claims, unknown evidence IDs, unknown source IDs, and count overflow; focused negative test passes | pass |
| Read-only built-in research authority | `assets/agents/research.toml` denies shell, mutation, terminal, and commit tools | pass |
| Prompt-injection/data boundary | Runtime prompt context labels retrieved text as untrusted data and denies authority changes | pass |
| Bounded disclosure and operator guidance | Prompt context exposes counts/scopes only; `architecture/agent.md` records limits and ownership | pass |

## 3. Production implementation evidence

The new runtime contract is in `src/research/runtime.rs`. It is deliberately
host-side and typed: the plan contains only bounded task scopes, reports carry
source/evidence/claim records, and the final report requires explicit source
and evidence relationships. The existing `ResearchCoordinator` continues to
own artifact persistence, source adapters, extraction, contradiction checks,
and verification. No second scheduler or research-specific execution pool was
introduced.

`ToolRegistryOptions.workspace_root` is populated by the production session
factory from `ExecutionContext`. The registry therefore constructs the
research service with the canonical workspace root before the turn starts.

## 4. Verification executed

Local verification:

```text
cargo test -p codegg --lib research::runtime       # 4 passed
cargo test -p codegg --lib tool::research          # 4 passed
cargo test -p codegg --lib research::              # 93 passed
cargo test -p codegg --lib search_backend::         # 64 passed
cargo test --test subagent                          # 22 passed
cargo check -p codegg --lib                         # passed
cargo check --workspace                             # passed
cargo fmt --all -- --check                          # passed
python3 scripts/check_builtin_agents.py             # passed
python3 scripts/generate_builtin_agents.py --check  # passed
python3 scripts/check_daemon_cwd_usage.py           # passed
python3 scripts/check_scheduler_bypass.py           # passed
git diff --check                                    # passed
```

No live provider, paid API key, browser automation, or external search
service was required. These are intentionally outside the milestone's closure
evidence.

## 5. Invariant review

- Workspace/service identity is explicit on the daemon path.
- Research remains read-only by default and cannot widen child authority.
- The ordinary task tool, scheduler, permission checker, AgentLoop,
  cancellation, and event ownership remain authoritative.
- Fan-out, sources, evidence, claims, and prompt text are bounded.
- Child roles are evidence-producing scopes; the parent owns synthesis and
  citation validation.
- Retrieved content is treated as untrusted data rather than instructions.

## 6. Failure and recovery review

The existing research service retains explicit source/provider failures and
verification failures rather than fabricating a successful answer. Runtime
planning is deterministic and cannot create an unbounded tree. Normal turn
cancellation and child cancellation remain on M003's shared delegation path;
no alternate research cancellation path was added. Restart and durable
AgentRun recovery remain later roadmap responsibilities.

## 7. Migration and compatibility review

The existing `ResearchTool` API and `ResearchService` constructors remain
available for generic callers, TUI commands, and tests. The cwd constructor is
isolated to compatibility/test use; the production session registry receives
an explicit workspace root. Existing research mode/depth inputs and search
backends are unchanged.

## 8. Security review

The built-in research agent no longer asks for shell, filesystem mutation,
terminal, or commit authority. No new network or process execution path was
introduced. Source text is bounded before prompt/report projection, provider
credentials are not represented in the runtime report schema or progress
summary, and unknown citation references fail local validation.

## 9. Documentation and operations

`architecture/agent.md` documents runtime dispatch, bounded roles, explicit
workspace ownership, evidence validation, and untrusted retrieved text. The
generated built-in agent assets and planning registry were regenerated and
updated in the implementation commit.

## 10. Unresolved findings (severity: critical/high/medium/low)

None at critical, high, or medium severity. Low-risk limitations are
intentional and documented: live-provider reliability, browser automation,
web-scale crawling, persistent knowledge graphs, and durable AgentRun restart
recovery remain outside M005.

## 11. Roadmap disposition

Milestone 005 is closed. M006 and M007 were already independently ready and
remain ready. The blocked-work audit found no newly unblocked plan:

- M008 still requires strict M007 closure.
- M009 still requires M007 and M008 (and later reasoning integration).
- M010 still requires M006 and M009.
- M011 still requires strict closure of M004 through M010.

No corrective pass is required.

## 12. Registry updates

- Marked the implementation plan `implemented`.
- Added this closure record with implementation commit `e3db48c`.
- Removed M005 from dependency-ready implementation plans.
- Marked the roadmap as M005 closed with M006–M007 ready.
- Audited every registered blocked plan; none became ready from M005 alone.
