# Agent Runtime, Model Adaptation, and ACP Milestone 011 — Corrective Status

Status: conditionally closed

Supersedes the strict disposition in:

- `plans/closure/agent-runtime-model-adaptation-acp/011-status.md`

Reviewed repository head: `7d8657e60aad85f677144b1bd0e7fb5d2929faa3`

Corrective roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md`

Next dependency-ready plan:

- `plans/implementation/agent-runtime-model-adaptation-acp/012-acp-turn-lifecycle-and-correlation-correctness.md`

## 1. Executive finding

Milestones 001–011 delivered substantial production architecture and remain valid historical implementation evidence. Strict subsystem closure is nevertheless withdrawn because a post-closure production-path audit identified unresolved correctness gaps that are not represented accurately by the original M011 claim that no in-scope high or medium finding remained.

The subsystem is conditionally closed pending Milestones 012–017. No broad redesign is required. The corrective work is bounded to ACP lifecycle correlation, specialized runtime finalization/research coordination, prompt/context identity convergence, adapter-driven reasoning safety, descendant admission/cancellation, explicit tool execution context, and truthful final evidence.

## 2. Findings invalidating strict closure

### ACP lifecycle and correlation

- `session/cancel` and `session/close` can be acknowledged before a native turn ID is known without retaining the same pending-cancellation behavior used by `$/cancel_request`.
- Active prompt binding and projection terminal handling rely too heavily on session-scoped event observation and do not consistently require one exact native turn correlation.
- `session/load` relabels stored text parts as assistant chunks rather than preserving supported user/assistant roles.

Impact: cancellation can be lost in the submit/start race, and stale or neighboring same-session events can bind or terminate the wrong ACP prompt.

### Specialized runtime finalization

- Security local report validation exists as a helper but is not authoritative in the production terminal path.
- Research runtime types and plans exist, but production remains prompt-led: planned children are not host-coordinated into a typed evidence ledger and final report validation is not authoritative.

Impact: provider schema noncompliance, unsupported findings, fabricated citations, or insufficient research evidence can bypass the intended host-owned completion contract.

### Prompt/context identity

- Root execution appends memory, specialized context, goals, LSP, Git, and an additional plan-mode contract after `PromptCompiler` returns.
- These blocks are outside compiler block identity/fingerprint, and plan guidance is duplicated.

Impact: the declared canonical compiler/cache identity does not fully describe the provider-effective prompt.

### Reasoning and adapter authority

- Private reasoning truncation can slice at a non-UTF-8 character boundary near the byte limit.
- OpenAI-compatible Laguna behavior is selected through model-name substring checks even though the adapter TOML declares the required transforms.

Impact: valid multibyte streams can panic, and adapter resolution is not the sole authority for model-specific request behavior.

### Descendant and workspace ownership

- Active-descendant capacity is checked before enqueue but incremented later in worker execution, permitting concurrent oversubscription.
- Cancellation is primarily pool-global rather than clearly root-lineage scoped.
- Native tool execution context still derives cwd from `std::env::current_dir()` instead of explicit workspace execution context.

Impact: configured bounds and multi-project execution ownership are weaker than closure documentation claims.

## 3. Severity and disposition

| Finding group | Severity | Disposition |
|---|---|---|
| ACP pre-turn cancellation and turn correlation | medium | M012 owns correction |
| Security/research production finalization | medium | M013 owns correction |
| Prompt/context fingerprint incompleteness and duplicate plan contract | medium | M014 owns correction |
| UTF-8 reasoning truncation and model-substring transform authority | medium | M015 owns correction |
| Atomic descendant admission, lineage cancellation, explicit tool cwd | medium | M016 owns correction |
| Cross-milestone evidence and registry reconciliation | closure requirement | M017 owns independent review |

No known critical finding was identified. Existing durable AgentRun, worktree-isolation, and team-authorization deferrals remain outside this corrective scope.

## 4. Preserved accomplishments

The corrective disposition does not reject the completed architecture:

- root and child turns use the shared prompt compiler and resolved tool surface;
- nested delegation is functional through the shared pool;
- security/research preparation contracts exist;
- bounded observable recovery exists;
- strict generated adapter assets exist;
- provider-private reasoning has a private internal representation;
- `ContextPlan` preserves provider message chronology;
- ACP v1 stdio transport and native daemon attachment exist.

Corrective milestones must amend these seams rather than replace them.

## 5. Verification status

The original M011 focused suites and static guards remain useful historical evidence. They do not prove the missing production finalization, exact ACP race behavior, atomic admission, UTF-8 boundary safety, or complete prompt fingerprinting identified above.

The original broad workspace library command was not green and the project-catalog guard was stale at that reviewed head. Milestone 017 must report focused and broad verification truthfully and must not treat an abort, skipped command, missing target, or unrelated failure as a pass.

## 6. Planning and registry disposition

- Original M011 is retained as historical implementation/closure evidence.
- This corrective status governs current subsystem disposition.
- The corrective addendum is active.
- M012 is dependency-ready.
- M013–M017 are registered behind explicit predecessor closure.
- The subsystem may return to strict `closed` only through an independent M017 closure record with no unresolved high or medium finding.

## 7. Stop conditions

Corrective implementation must stop rather than expand scope if it requires:

- durable AgentRun persistence/restart recovery;
- worktree-native mutation isolation;
- final team/principal authorization;
- ACP v2 or editor-specific extensions;
- browser automation, external scanners, or mandatory live providers;
- a broad scheduler, projection, storage, or release redesign.
