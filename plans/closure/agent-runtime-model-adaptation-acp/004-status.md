# Agent Runtime, Model Adaptation, and ACP Milestone 004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-runtime-model-adaptation-acp/004-specialized-security-review-runtime.md`

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-004--specialized-security-review-runtime`

Repository baseline reviewed: `672479726f1c79bbc931d70f084cd1649e8b2ed4`

Implementation commits:

- `193db6de` — host-side security-review runtime preparation, bounded evidence contract, and planning closure

## 1. Executive finding

Milestone 004 is closed. The resolved `security_review` runtime kind now
selects a host-owned deterministic preparation stage before the ordinary agent
loop. It discovers the bounded working-tree diff, runs the existing local
security preflight and conservative synthesis, fingerprints the prepared
bundle, injects compact evidence into the canonical prompt, and derives
security-aware LSP context from the same scope. Provider streaming, tool
execution, permissions, cancellation, and scheduler ownership remain on the
ordinary runtime path.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Runtime dispatch by resolved kind | `src/agent/turn_runtime.rs` checks `AgentRuntimeKind::SecurityReview` before prompt assembly | pass |
| Bounded typed evidence bundle | `src/security/runtime.rs` defines input, bundle, records, gaps, coverage, and fingerprint | pass |
| Changed-file/hunk deterministic preflight | `prepare_security_review` calls `run_security_review_workflow` with bounded output limits | pass |
| Marker-only prompts remain distinct | Existing workflow synthesis plus injected prompt contract explicitly separates prompts from findings | pass |
| Schema/evidence validation | `validate_report` rejects findings outside prepared targets or without evidence/reasoning | pass |
| LSP security evidence | Prepared targets populate `LspAgentContextInput` with `security_review_mode`; existing production LSP assembler handles the request when available | pass |
| Read-only ordinary authority | No new tools, scheduler, process execution, or mutation path was introduced; security-review asset denies mutation | pass |
| Bounded projections | Prompt receives compact check summaries, counts, gaps, and fingerprint; no raw scanner detail is injected | pass |
| Optional evidence gaps | Bundle records unavailable/empty optional evidence as bounded gaps | pass |
| Specialist delegation | Existing M003 shared pool, depth, authority, and explicit target policy remain the sole child path; this runtime does not bypass or broaden it | pass |

## 3. Production implementation evidence

`SecurityEvidenceBundle::prompt_context` is the host-to-model boundary. It
limits records and text, states the marker/finding contract, and exposes the
bundle fingerprint. `prepare_security_review` reuses the established diff,
preflight, hunk, and conservative synthesis implementation instead of
duplicating scanners. Security turns derive LSP changed-file/hunk input from
the prepared targets, so the deterministic and semantic evidence scopes do
not diverge.

## 4. Verification executed

Local commands:

```text
cargo fmt --all
cargo check -p codegg --lib
cargo test -p codegg --lib security::runtime
```

Results: all passed. The focused runtime test binary reported 2 passed tests.
The linker emitted only the existing macOS `__eh_frame` warning.

The pre-existing security workflow tests remain the production-shaped coverage
for diff discovery, preflight, LSP enrichment, receipt projection, and
cancellation. They were not changed by this milestone. A final verification
run is recorded in the implementation commit before publication.

## 5. Invariant review

- Security review remains read-only and does not gain mutation or shell
  authority.
- Host preparation is mandatory for resolved security-review turns; the model
  is not responsible for remembering baseline scans.
- Markers and diagnostics are prompts unless the workflow has additional
  evidence, and model findings must point into the prepared scope.
- Evidence sent to the model is bounded and source-located without raw secret
  or scanner-body projection.
- The standard AgentLoop, permission checker, tool surface, cancellation, and
  scheduler remain the authority boundaries.

## 6. Failure and recovery review

Review-scope/workflow failure prevents the specialized turn from starting and
returns an error. Empty scope is represented as an explicit evidence gap.
The existing workflow remains fail-open for optional LSP/enrichment evidence.
Once the normal AgentLoop starts, cancellation and terminal handling are
unchanged and owned by the existing turn runtime. No partial prepared bundle
is published as a completed review.

## 7. Migration and compatibility review

The existing `/security-review` workflow and generic security-tool callers are
unchanged. The new behavior is selected only by resolved `runtime_kind`.
Agent files and protocol DTOs remain compatible; no storage migration or
protocol variant was added. Custom agents inheriting `security-review` retain
the runtime kind through the existing registry merge behavior.

## 8. Security review

The new code is defensive-only. It does not generate exploits, perform network
scanning, invoke external vulnerability services, or add an offensive tool.
Path and authority checks remain under existing workspace and permission
policies. The prompt boundary contains summaries and identifiers, not full
source bodies or secret values.

## 9. Documentation and operations

`architecture/tool.md` now documents the runtime dispatch, evidence boundary,
fingerprint, LSP scope reuse, and validation semantics. Registry, roadmap,
implementation-plan, and closure records were updated together.

## 10. Unresolved findings

None at critical, high, or medium severity. Full durable AgentRun restart
recovery and mutation worktree isolation remain explicitly owned by later
roadmap work and are not Milestone 004 requirements.

## 11. Roadmap disposition

Milestone 004 is closed. Milestone 005 remains ready because it has the same
closed M001–M003, runtime-assets M004, and session-projections M012
dependencies. M006 and M007 remain independently ready. M008–M010 retain
their stated predecessor blockers. M011 remains blocked because closing M004
alone does not satisfy its M005–M010 dependency set.

## 12. Registry updates

- Marked the implementation plan implemented and linked this closure record.
- Removed M004 from dependency-ready plans.
- Added M004 to recently closed work.
- Updated the subsystem roadmap current status to M004 closed / M005 ready.
- Audited every registered blocked plan: no plan became newly unblocked from
  this milestone alone, and no corrective pass is required.
